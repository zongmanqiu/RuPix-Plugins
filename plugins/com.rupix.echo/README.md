# com.rupix.echo

诊断插件：连接宿主管道，读取一条请求帧，把请求 JSON 原样放进响应文本。
它没有业务功能，只用来验证：

- 宿主能启动插件进程；
- 插件能拿到 `--rupix-pipe` / `--rupix-protocol`；
- 帧格式（u32 LE 长度 + body）与 JSON 响应能往返；
- **设置通道**：manifest 声明 `settings_ui` + `settings.read/write`，宿主「插件」页该行的
  齿轮会以 `action=settings` 拉起本插件，弹出「回显结果转为大写？」的对话框，
  答案写进 `settings.echo.upper`（`"1"` / `"0"`），此后每次 `run` 宿主都会把设置下发回来。

构建：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File build.ps1
```

产物写到工作区 `.temp/plugin-dist/com.rupix.echo/`，根 `build.ps1` 会镜像到
`.build/docs/com.rupix.echo_0.1.1.zip`（连同本 README 与 LICENSE）。

宿主冒烟：

```powershell
cd <宿主>\main
cargo run --release --example plugin_smoke -- ..\.temp\plugin-dist\com.rupix.echo
```
