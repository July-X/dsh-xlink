# 发布流水线

`.github/workflows/desktop-release.yml` 负责生成并发布 Intel macOS 与 Windows x86_64 安装包。发布只接受 `main` 可达的 `desktop-v<version>` tag，或从 `main` 手动 dispatch。需要复用跨版本的 Rust 编译缓存时，推荐在 Actions 页面选择 `main` 手动 dispatch；推送 tag 的方式仍然保留。

## 时序

1. `preflight` 在 Intel macOS runner 上校验来源 commit、tag 格式，以及 `package.json` 与 `src-tauri/tauri.conf.json` 的版本一致性。
2. `quality` 与两个平台的 `build` 在预检通过后并行启动。质量门禁继续执行 UI 测试、UI 构建、bundle 预算、Rust 格式检查、测试和 Clippy；构建矩阵只包含 `macos-15-intel` 与 `windows-latest`，`max-parallel: 2`。
3. 平台 `build` 只执行签名构建并上传 Actions artifact，不创建或修改 GitHub Release。这样质量检查不会留下半成品，两个平台也不再争用同一份 `latest.json`。
4. `publish` 只在预检、质量门禁和两个平台构建全部成功后运行。它下载两个 artifact，校验五个安装/更新文件各有且只有一份，由 `scripts/generate-updater-manifest.mjs` 生成 `latest.json`，再一次性上传全部资产。
5. 新 Release 先以 draft 形式接收完整资产，上传结束后才转换为正式 release；最终 `draft=false`、`prerelease=false`，因此 `releases/latest/download/latest.json` 在发布完成时才对用户可见。

手动 dispatch 没有现成 tag 时，`publish` 会把当前 `main` commit 创建为 `desktop-v<version>` tag；tag 已存在时仅复用对应的 draft Release，正式 Release 会直接拒绝覆盖。版本文件未同步、tag 不在 `main` 或任一构建产物缺失都会在发布前失败。

## 缓存与构建

- `setup-node` 为 pnpm store 建立缓存，依赖使用 `pnpm install --frozen-lockfile --prefer-offline`。
- `Swatinem/rust-cache@v2` 分别缓存质量 job 与 release job 的 Cargo registry、Git 依赖和编译结果；缓存 key 含 runner 系统与架构，避免 debug 检查产物污染 release target。缓存只在 `main` 分支保存：GitHub 对不同 tag 使用独立的缓存作用域，tag 之间无法复用彼此的缓存，继续保存这些缓存只会延长 job 收尾时间。
- `src-tauri/Cargo.toml` 使用 `lto = "thin"` 与 `codegen-units = 16`。这比完整 LTO 与单 codegen unit 更适合 CI 冷构建，同时保留 release 优化。
- artifact 使用零压缩上传。DMG、tar.gz 和安装包本身已经压缩，减少 runner 上的重复压缩时间。

首次从 `main` 手动发布时仍会经历依赖下载和编译；后续手动发布在 lockfile、Rust 工具链和 runner 未变化时可以复用缓存。直接推送新 tag 会进入新的缓存作用域，通常会走冷启动。runner 排队、缓存服务和网络波动不属于 workflow 可控的构建时间。

## updater 文件

发布脚本会生成以下四个平台键：

```json
{
  "platforms": {
    "darwin-x86_64": {
      "signature": "...",
      "url": "...app.tar.gz"
    },
    "darwin-x86_64-app": {
      "signature": "...",
      "url": "...app.tar.gz"
    },
    "windows-x86_64": {
      "signature": "...",
      "url": "...-setup.exe"
    },
    "windows-x86_64-nsis": {
      "signature": "...",
      "url": "...-setup.exe"
    }
  }
}
```

四个平台键是 Tauri 的兼容别名：每个平台的两个键都指向同一份 updater archive 或 NSIS 安装包。macOS 构建上传前，以及 publish 下载 artifact 后，都会调用 `scripts/normalize-release-assets.mjs`，把 Tauri 可能生成的未带版本号的 updater archive 和签名按 DMG 文件名规范化。脚本要求 DMG、macOS updater archive、macOS 签名各找到一份，并确认 DMG 文件名包含当前版本；manifest 脚本再校验五个发布资产的版本。签名内容从对应 `.sig` 文件读取，不经过日志输出。

## 排障

- 质量 job 失败时，构建 job 可能已经完成，但 `publish` 会保持跳过，不会创建正式 Release。
- 如果构建变慢，先确认发布是否从 `main` 手动 dispatch，再查看 `Cache Rust release dependencies` 与 `Install JavaScript dependencies` 的命中情况，区分缓存冷启动、依赖编译和 runner 排队耗时。
- 如果 `latest.json` 生成失败，先检查 artifact 中是否只有一份 `.dmg`、`.app.tar.gz`、`.app.tar.gz.sig`、`*-setup.exe` 与 `*-setup.exe.sig`，以及各文件名是否带当前版本。
- 如果 updater 返回 404，确认最终 Release 是正式 release，且 `latest.json` 已与五个资产一起上传；不要把 `draft` 或 `prerelease` 留为 true。
