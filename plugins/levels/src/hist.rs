//! 像素分布统计：共享内存里的 RGBA8 整图 → R / G / B / 亮度四组 256 箱。
//!
//! 纪律三条（与 `checkup` 同族）：
//! 1. **如实交代来路**。宿主下发的像素不一定是原图全量：超过 8 MP 时它会**等距抽样**
//!    （每 N 像素取 1 点，只挑不混），首帧也可能只是视口档。报告里必须写清"这份像素
//!    对应原图多少"——0.1.6 之前一律写"全量，未抽稀"，在 561×561 的预览档上就是假话。
//! 2. **全透明像素不算数**。屏幕上那些位置画的是棋盘格，不是图像颜色，
//!    把它们算进亮度会把任何带透明边的图拉出一大坨假的"死黑"。
//! 3. **公式写死**：亮度用 Rec.601 的整数近似 `(77R + 150G + 29B) >> 8`
//!    （与 `checkup` 的清晰度段同一份灰度定义，两张报告可以横着比）。

use crate::shm::Raster;

/// 这份像素的来路：原图多大、抽没抽样。
///
/// 宿主在请求里给 `source = {width, height}` 与 `sample_step`；两者都不给（老宿主、
/// 或本来就是全量）= [`Input::FULL`]，报文与 0.1.6 逐字相同。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Input {
    pub source: Option<(u32, u32)>,
    pub step: u32,
}

impl Input {
    /// 全量、未抽样（也是"宿主没说"时的默认：宁可说全量，也不替宿主猜）。
    pub const FULL: Input = Input {
        source: None,
        step: 1,
    };

    /// 报告里那半句"这份统计覆盖原图多少"。
    ///
    /// 三态，缺一态就会撒谎：
    /// - 全量：`source` 与图尺寸一致、且没抽样；
    /// - **抽样**：`step > 1`，说清原图多大、每几点取一、总共多少样本；
    /// - **预览档**：`step == 1` 但图比原图小（首帧视口档），说清"这是哪一档、怎么拿到更高"。
    pub fn coverage_note(&self, w: usize, h: usize) -> String {
        let Some((sw, sh)) = self.source else {
            // 宿主给了抽样却没给原图尺寸（协议里两个字段是分开的可选项）：
            // 仍要把"这是抽样"说出来，不能退回"全量"。
            return if self.step > 1 {
                format!(
                    "**抽样统计**：每 {} 像素取 1 点 ⇒ {w}×{h} 共 {} 个样本（宿主未给原图尺寸）",
                    self.step,
                    w as u64 * h as u64
                )
            } else {
                "全量，未抽稀".to_string()
            };
        };
        let src_total = sw as u64 * sh as u64;
        let got = w as u64 * h as u64;
        if src_total == 0 {
            return "全量，未抽稀".to_string();
        }
        let cover = got as f64 * 100.0 / src_total as f64;
        if self.step > 1 {
            format!(
                "**抽样统计**：原图 {sw}×{sh}，每 {} 像素取 1 点 ⇒ {w}×{h} 共 {got} 个样本，\
                 覆盖原图 {cover:.2}%；每个样本都是原图上真实存在的原始像素，未做平均",
                self.step
            )
        } else if got < src_total {
            format!(
                "**预览档**：原图 {sw}×{sh}，本次只拿到 {w}×{h}（覆盖 {cover:.2}%）；\
                 放大到 100% 再分析可获得更高档位"
            )
        } else {
            "全量，未抽稀".to_string()
        }
    }
}

pub const BINS: usize = 256;
pub const CH_R: usize = 0;
pub const CH_G: usize = 1;
pub const CH_B: usize = 2;
pub const CH_L: usize = 3;
pub const CH_NAMES: [&str; 4] = ["R", "G", "B", "亮度"];

/// 亮度公式。单独留一个函数是为了让它能被单测直接钉住，而不是藏在循环里。
pub fn luma(r: u8, g: u8, b: u8) -> u8 {
    ((77u32 * r as u32 + 150u32 * g as u32 + 29u32 * b as u32) >> 8) as u8
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Histogram {
    /// 四组 × 256 箱；`[3]` 是亮度。
    pub bins: [[u32; BINS]; 4],
    /// 原图尺寸（报告里要印"统计范围"，不许拿 0 占位糊过去）。
    pub width: usize,
    pub height: usize,
    /// 真正参与统计的像素数（alpha > 0）。
    pub counted: u64,
    /// 因 alpha == 0 跳过的像素数。
    pub transparent: u64,
    /// 四组各自的和，用来算均值，避免再遍历一遍箱。
    pub sum: [u64; 4],
}

impl Histogram {
    pub fn accumulate(raster: &Raster) -> Self {
        let mut out = Histogram {
            bins: [[0u32; BINS]; 4],
            width: raster.width,
            height: raster.height,
            counted: 0,
            transparent: 0,
            sum: [0; 4],
        };
        for y in 0..raster.height {
            for x in 0..raster.width {
                let [r, g, b, a] = raster.pixel(x, y);
                if a == 0 {
                    out.transparent += 1;
                    continue;
                }
                let l = luma(r, g, b);
                let vals = [r, g, b, l];
                for ch in 0..4 {
                    out.bins[ch][vals[ch] as usize] += 1;
                    out.sum[ch] += vals[ch] as u64;
                }
                out.counted += 1;
            }
        }
        out
    }

    pub fn mean(&self, ch: usize) -> f64 {
        if self.counted == 0 {
            return 0.0;
        }
        self.sum[ch] as f64 / self.counted as f64
    }

    /// 中位数所在的箱：累计首次达到半数。空图返回 0。
    pub fn median_bin(&self, ch: usize) -> usize {
        if self.counted == 0 {
            return 0;
        }
        let half = (self.counted + 1) / 2;
        let mut acc = 0u64;
        for (i, n) in self.bins[ch].iter().enumerate() {
            acc += *n as u64;
            if acc >= half {
                return i;
            }
        }
        BINS - 1
    }

    /// 峰值箱。**平票取箱号小的那个**——判词不许依赖遍历方向或哈希次序。
    pub fn peak_bin(&self, ch: usize) -> usize {
        let mut best = 0usize;
        for i in 1..BINS {
            if self.bins[ch][i] > self.bins[ch][best] {
                best = i;
            }
        }
        best
    }

    /// 四条曲线共用同一纵轴时的满格值。
    pub fn global_peak(&self) -> u32 {
        self.bins
            .iter()
            .flat_map(|row| row.iter())
            .copied()
            .max()
            .unwrap_or(0)
    }

    /// 亮度落在 `[lo, hi]` 的像素数（含两端）——暗部与高光溢出都走这里。
    pub fn luma_range(&self, lo: usize, hi: usize) -> u64 {
        self.bins[CH_L][lo..=hi]
            .iter()
            .map(|n| *n as u64)
            .sum()
    }

    pub fn total_pixels(&self) -> u64 {
        self.counted + self.transparent
    }

    /// 结果窗里那段可复制的 Markdown。`with_image` 只是"本次要不要出图"的说明，
    /// 统计图本身与它无关——设不设置都不影响任何数字。
    ///
    /// 便捷壳：不传来路 = 全量（0.1.6 及更早的报文，逐字不变）。
    ///
    /// 只给单测与内部默认用——生产路径一律 `report_with`（要带 `input_of` 读到的来路）。
    #[cfg(test)]
    pub fn report(&self, with_image: bool) -> String {
        self.report_with(with_image, Input::FULL)
    }

    /// 带**来路**的报告。`input` 说清这份像素对应原图多少，报告才可能不撒谎。
    pub fn report_with(&self, with_image: bool, input: Input) -> String {
        let pct = |n: u64| -> String {
            if self.counted == 0 {
                "0.00%".to_string()
            } else {
                format!("{:.2}%", n as f64 * 100.0 / self.counted as f64)
            }
        };
        let dark = self.luma_range(0, 5);
        let bright = self.luma_range(250, 255);
        let means = [self.mean(CH_R), self.mean(CH_G), self.mean(CH_B)];
        let hi = means.iter().copied().fold(f64::MIN, f64::max);
        let lo = means.iter().copied().fold(f64::MAX, f64::min);
        let delta = hi - lo;
        let dominant_idx = means.iter().position(|m| *m == hi).unwrap_or(CH_R);
        let dominant = CH_NAMES[dominant_idx];
        let tint_word = ["红色", "绿色", "蓝色"][dominant_idx];
        let tint = if delta > 12.0 {
            format!("通道差 Δ={delta:.1} > 12，{dominant} 通道整体偏高，画面偏{tint_word}")
        } else {
            format!("通道差 Δ={delta:.1} ≤ 12，未见明显偏色")
        };
        let chart = if with_image {
            "已生成 640×360 PNG（四条平滑曲线叠加在同一绘图区，只描线、不给曲线下面积上色，\
             交叉处也能分清），由宿主开**新的看图窗口**显示；红=R、绿=G、蓝=B、白=亮度，\
             四条曲线共用同一纵轴"
        } else {
            "本次按设置只出文本，未生成图形（设置里打开「同时生成分布曲线图」即可看图）"
        };
        let peak = self.peak_bin(CH_L);
        let coverage = input.coverage_note(self.width, self.height);
        format!(
            "## 色阶分析\n\n\
             - **统计范围**：{w}×{h} 共 {total} 像素，**实际统计 {counted} 点**（{coverage}），\
             跳过全透明 {transparent} 点。\n\
             - **亮度**（Rec.601 整数近似 `(77R+150G+29B)>>8`）：均值 {mean:.1} / \
             中位数箱 {median} / 峰值箱 {peak}（{peak_n} 点）。\n\
             - **暗部溢出**（亮度 0–5）：{dark} 点（{dark_pct}）；\
             **高光溢出**（亮度 250–255）：{bright} 点（{bright_pct}）。\n\
             - **三通道均值**：R {mr:.1} / G {mg:.1} / B {mb:.1} → {tint}。\n\
             - **读法**：柱子贴左边 = 暗部糊成死黑；贴右边 = 高光死白、细节丢了；\
             山形居中且两头不贴边 = 明暗都有内容。\n\
             - **纵轴**：四格共用同一纵轴，满格 = 四组里最大的那一箱（{gpeak} 点，\
             占实际统计 {counted} 点的 {gpeak_pct}）；图上五条刻度（基线 0.00% 起）标的是占全图的百分比，\
             理论上限 100%（所有可见像素落在同一箱）。\n\
             - **图形**：{chart}。\n",
            w = self.width,
            h = self.height,
            total = self.total_pixels(),
            counted = self.counted,
            transparent = self.transparent,
            coverage = coverage,
            mean = self.mean(CH_L),
            median = self.median_bin(CH_L),
            peak = peak,
            peak_n = self.bins[CH_L][peak],
            dark = dark,
            dark_pct = pct(dark),
            bright = bright,
            bright_pct = pct(bright),
            mr = means[0],
            mg = means[1],
            mb = means[2],
            tint = tint,
            gpeak = self.global_peak(),
            gpeak_pct = pct(self.global_peak() as u64),
            chart = chart,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raster(px: &[[u8; 4]], w: usize, h: usize) -> Raster {
        Raster {
            width: w,
            height: h,
            rgba: px.iter().flatten().copied().collect(),
        }
    }

    #[test]
    fn luma_formula_matches_rec601_endpoints() {
        assert_eq!(luma(0, 0, 0), 0);
        assert_eq!(luma(255, 255, 255), 255);
        assert_eq!(luma(255, 0, 0), 76, "纯红的 Rec.601 灰度 = 76");
    }

    #[test]
    fn pure_red_lands_where_it_should() {
        let r = raster(&[[255, 0, 0, 255]; 2], 2, 1);
        let h = Histogram::accumulate(&r);
        assert_eq!(h.counted, 2);
        assert_eq!(h.bins[CH_R][255], 2);
        assert_eq!(h.bins[CH_G][0], 2);
        assert_eq!(h.bins[CH_B][0], 2);
        assert_eq!(h.bins[CH_L][76], 2, "亮度箱由公式算，不是复制 R 通道");
        assert_eq!(h.global_peak(), 2);
        assert_eq!(h.peak_bin(CH_R), 255);
    }

    #[test]
    fn fully_transparent_pixels_are_excluded_not_counted_as_black() {
        let r = raster(&[[10, 10, 10, 0], [200, 200, 200, 255]], 2, 1);
        let h = Histogram::accumulate(&r);
        assert_eq!(h.counted, 1);
        assert_eq!(h.transparent, 1);
        assert_eq!(h.bins[CH_L][10], 0, "透明像素不许假装是暗部");
        assert_eq!(h.bins[CH_L][200], 1);
    }

    #[test]
    fn peak_ties_go_to_the_lowest_bin_and_median_is_a_real_bin() {
        let r = raster(
            &[[10, 20, 30, 255], [200, 210, 220, 255]],
            2,
            1,
        );
        let h = Histogram::accumulate(&r);
        assert_eq!(h.peak_bin(CH_R), 10, "平票不许依赖遍历方向");
        assert_eq!(h.median_bin(CH_R), 10, "两点的中位数落在较暗那箱");
        assert_eq!(h.luma_range(0, 5), 0);
        // 第二个像素 (200,210,220) 的亮度是 208，不是 B 通道的 220——
        // 这条钉的是"亮度箱由公式算"，不是把某个通道复制一遍。
        assert_eq!(h.bins[CH_L][208], 1);
        assert_eq!(h.bins[CH_B][220], 1);
        assert_eq!(h.luma_range(200, 255), 1);
    }

    #[test]
    fn clipping_counts_use_the_luma_bins() {
        let r = raster(&[[0, 0, 0, 255], [255, 255, 255, 255]], 2, 1);
        let h = Histogram::accumulate(&r);
        assert_eq!(h.luma_range(0, 5), 1);
        assert_eq!(h.luma_range(250, 255), 1);
        assert_eq!(h.mean(CH_L), 127.5);
    }

    #[test]
    fn report_states_full_coverage_and_the_chart_switch() {
        let r = raster(&[[255, 0, 0, 255]; 4], 2, 2);
        let h = Histogram::accumulate(&r);
        let on = h.report(true);
        assert!(on.contains("全量，未抽稀"), "{on}");
        assert!(on.contains("640×360 PNG"), "{on}");
        assert!(on.contains("偏红色"), "{on}");
        let off = h.report(false);
        assert!(off.contains("未生成图形"), "{off}");
        // 关掉图形不许改动任何一个数字。
        assert_eq!(
            on.split("三通道均值").next(),
            off.split("三通道均值").next()
        );
    }

    #[test]
    fn report_names_the_axis_denominator_and_the_theoretical_ceiling() {
        // 2x2 纯红：四组峰值箱都是 4 点，counted=4 ⇒ 满格 100.00%。
        let r = raster(&[[255, 0, 0, 255]; 4], 2, 2);
        let on = Histogram::accumulate(&r).report(true);
        assert!(on.contains("4 点，占实际统计 4 点的 100.00%"), "{on}");
        assert!(on.contains("理论上限 100%"), "{on}");
        // 关掉图形同样不许改动任何一个纵轴数字。
        let off = Histogram::accumulate(&r).report(false);
        assert!(off.contains("4 点，占实际统计 4 点的 100.00%"), "{off}");
    }

    /// ★ 三态来路必须各说各话。0.1.6 之前一律印"全量，未抽稀"——
    /// 在 561×561 的预览档上那是假话，在抽样图上是更严重的假话。
    #[test]
    fn coverage_note_tells_full_sample_and_preview_apart() {
        let r = raster(&[[10, 20, 30, 255]; 4], 2, 2);
        let h = Histogram::accumulate(&r);

        // ① 全量：宿主给的原图尺寸与这份像素一致。
        let full = h.report_with(true, Input { source: Some((2, 2)), step: 1 });
        assert!(full.contains("全量，未抽稀"), "{full}");

        // ② 抽样：原图 4080²、这份 1020²、每 4 像素取 1 点 ⇒ 覆盖 6.25%。
        //    覆盖率那半句直接测纯函数（不必为它造一张真的 1020² 图）。
        let note = Input {
            source: Some((4080, 4080)),
            step: 4,
        }
        .coverage_note(1020, 1020);
        assert!(note.contains("抽样统计"), "{note}");
        assert!(note.contains("每 4 像素取 1 点"), "{note}");
        assert!(note.contains("6.25%"), "1020²/4080² 应报 6.25%：{note}");
        let sampled = h.report_with(
            true,
            Input {
                source: Some((4080, 4080)),
                step: 4,
            },
        );
        assert!(sampled.contains("抽样统计"), "{sampled}");
        assert!(!sampled.contains("全量"), "抽样图不许再声称全量：{sampled}");

        // ③ 预览档：没抽样，但只有原图的一小块 —— 这正是"第一次能、第二次不能"那次的样子。
        let preview = h.report_with(
            true,
            Input {
                source: Some((4080, 4080)),
                step: 1,
            },
        );
        assert!(preview.contains("预览档"), "{preview}");
        assert!(preview.contains("4080×4080"), "{preview}");
        assert!(
            preview.contains("放大到 100%"),
            "要说清怎么拿到更高档：{preview}"
        );

        // ④ 宿主只给了 step、没给 source：仍须承认是抽样，不能退回"全量"。
        let bare = h.report_with(
            true,
            Input {
                source: None,
                step: 8,
            },
        );
        assert!(bare.contains("抽样统计"), "{bare}");
        assert!(!bare.contains("全量"), "{bare}");
    }
}
