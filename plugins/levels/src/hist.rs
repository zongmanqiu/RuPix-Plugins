//! 像素分布统计：共享内存里的 RGBA8 整图 → R / G / B / 亮度四组 256 箱。
//!
//! 纪律三条（与 `checkup` 同族，但这里是**全量**统计）：
//! 1. **不抽稀**。宿主下发上限 8 MP，256 个箱的整数累加在这个量级是毫秒级，
//!    没必要用采样换速度；报告里如实写"全量，未抽稀"。
//! 2. **全透明像素不算数**。屏幕上那些位置画的是棋盘格，不是图像颜色，
//!    把它们算进亮度会把任何带透明边的图拉出一大坨假的"死黑"。
//! 3. **公式写死**：亮度用 Rec.601 的整数近似 `(77R + 150G + 29B) >> 8`
//!    （与 `checkup` 的清晰度段同一份灰度定义，两张报告可以横着比）。

use crate::shm::Raster;

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
    pub fn report(&self, with_image: bool) -> String {
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
        format!(
            "## 色阶分析\n\n\
             - **统计范围**：{w}×{h} 共 {total} 像素；**实际统计 {counted} 点（全量，未抽稀）**，\
             跳过全透明 {transparent} 点。\n\
             - **亮度**（Rec.601 整数近似 `(77R+150G+29B)>>8`）：均值 {mean:.1} / \
             中位数箱 {median} / 峰值箱 {peak}（{peak_n} 点）。\n\
             - **暗部溢出**（亮度 0–5）：{dark} 点（{dark_pct}）；\
             **高光溢出**（亮度 250–255）：{bright} 点（{bright_pct}）。\n\
             - **三通道均值**：R {mr:.1} / G {mg:.1} / B {mb:.1} → {tint}。\n\
             - **读法**：柱子贴左边 = 暗部糊成死黑；贴右边 = 高光死白、细节丢了；\
             山形居中且两头不贴边 = 明暗都有内容。\n\
             - **纵轴**：四格共用同一纵轴，满格 = 四组里最大的那一箱（{gpeak} 点）。\n\
             - **图形**：{chart}。\n",
            w = self.width,
            h = self.height,
            total = self.total_pixels(),
            counted = self.counted,
            transparent = self.transparent,
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
}
