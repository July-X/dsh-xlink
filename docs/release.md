# 发布流水线

`.github/workflows/desktop-release.yml` 负责生成并发布 Intel macOS 与 Windows x86_64 安装包。发布仍只接受 `main` 可达的 `desktop-v<version>` tag，或从 `main` 手动 dispatch。

## 时序

1. `quality` job 在 Intel macOS runner 上执行 UI 测试、UI 构建与 bundle 预算检查，再执行 Rust 格式检查、测试和 Clippy。
2. 两个平台的 `build` job 在质量门禁通过后同时启动。`max-parallel: 2` 与矩阵中的两个平台一一对应，不构建 Linux 或其他架构。
3. 每个平台由 `tauri-apps/tauri-action@v1` 完成 Tauri 构建、签名、上传安装包和 updater 文件。`releaseDraft: false`、`prerelease: false` 保持不变，使 `releases/latest/download/latest.json` 可用。

当前发布 job 保留 `needs: quality`。`tauri-action` 把构建和 Release 上传放在同一个 action 中，不能简单地让它在质量门禁之前运行，否则质量检查失败时仍可能留下已发布资产。

## 缓存

- `setup-node` 为 pnpm store 建立缓存，依赖仍使用 `pnpm install --frozen-lockfile --prefer-offline`。
- `Swatinem/rust-cache@v2` 为 Cargo registry、git 依赖和依赖编译结果建立缓存；它会清理过期产物及工作区自身产物，避免把旧 Tauri bundle 一起恢复。
- quality 与 release 使用不同的 `shared-key`，避免 debug 检查产物和 release 产物互相污染；key 同时包含 runner OS/架构以及 Cargo manifest/lockfile 与 Rust 环境信息。

首次使用新 key 时缓存为空，构建会较慢；后续版本在 lockfile 未变化时可复用依赖和 target。缓存恢复失败不会改变构建结果，只会回退到完整编译。

## Rust release profile

`src-tauri/Cargo.toml` 保留 `strip = true`，将完整 LTO/单 codegen unit 调整为 `lto = "thin"` 与 `codegen-units = 16`。这保留 release 优化和较小产物，同时显著降低 CI 冷构建的链接成本。若未来需要重新调整，应同时比较两个平台的构建时长、安装包体积、启动时间和 updater 签名产物。

## 排障

- 查看 job 的 `Cache Rust dependencies` 或 `Cache Rust release dependencies` 步骤确认命中状态。
- 如果两个平台再次串行，检查 workflow 矩阵的 `max-parallel` 是否仍为 `2`，并确认没有新增 job-level `needs`。
- 如果 `latest.json` 缺失或 updater 报 404，确认 Release 仍是正式 release，且 `createUpdaterArtifacts`、签名 secrets、`releaseDraft: false` 和 `prerelease: false` 未被修改。
- 如果缓存命中但耗时异常，优先比较 Cargo 编译步骤与 GitHub runner 排队时间；缓存不会跨 macOS 与 Windows 复用。
