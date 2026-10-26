# 发布流水线

`.github/workflows/desktop-release.yml` 负责生成并发布 Intel macOS 与 Windows x86_64 安装包。只发布这两个平台，不增加 Linux / Ubuntu runner、制品或文案。发布只接受已推送且位于 `main` 上的 `desktop-v<version>` tag；手动 dispatch 也必须从 `main` 发起，并已有指向该 commit 的正确 tag。

## 发布操作

明确要求发布即授权版本更新、检查、提交和推送，无需逐步再次确认。开发任务默认仅本地提交；没有发布或推送指令时不推远端。提交只暂存本次改动文件，提交前检查暂存区，避免卷入并行会话改动。

1. 同步 `package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml` 三处版本，准备发版正文。
2. 在待发布代码上完整执行一次 `npm run check`；发布 commit 必须全绿，不手工拼判据替代门禁。
3. 检查 `git log --oneline origin/main..HEAD`，向用户报告将推的提交、作者、变更内容及尚未实机验证的风险，并说明 tag push 会触发发布。
4. 提交并推送 `main`，在刚推送的 commit 上创建 tag，再推 tag 自动触发流水线：

   ```sh
   git fetch origin main --no-tags
   git tag desktop-v<version> <main-commit>
   git push origin desktop-v<version>
   ```

创建或更新 tag 后不再手动 dispatch 同一版本。手动 dispatch 只用于已有正确 tag、没有同版本 Run 正在执行的情况；不要借它创建缺失 tag。

### 同版本失败重试

先查失败 job 再修复。新 commit 重发同版本前，查询 `gh api repos/July-X/dsh-xlink/releases/tags/<tag>`，确认尚无已发布的正式 Release；正式发布过的 tag 不得删除或移动。仅失败且未正式发布时，删除远端旧 tag（`git push origin :refs/tags/desktop-v<version>`），在新的已推送 `main` commit 上重建并推送，自动触发新 Run。手动 dispatch 不能绕过 tag 与 `$GITHUB_SHA` 一致性检查。

## 发布说明落在哪

**GitHub Release 的正文是固定模板**——`scripts/generate-updater-manifest.mjs` 的 `releaseNotes()` 写死「支持平台 + 使用方式」，更新弹窗里显示的也是它。它**不是**逐版的变更清单：模板是脚本常量，流水线也不会去读 commit。

真正的发布说明写在**发版 commit 的正文**里（`chore(release): v<版本>`）。两边分工：commit 正文给维护者与 git 历史，模板给用户的更新提示。

写发版正文时**别漏「用户必须自己动手」的那类提醒**——它们不会出现在任何自动生成的正文里。已发生过的漏写：补丁的「应用 / 撤销」机制在 v0.4.7 整体删除，而「已打过补丁的用户升级后无法在 UI 上撤销」这条提醒**从未进过任何一版发布说明**（逐版核过：`v0.4.7` 正文是空的，此后各版正文与线上 Release 正文都没有它）。从 ≤v0.4.6 升上来的用户至今看不到这条提醒。

## 时序

1. `preflight` 在 Intel macOS runner 上校验来源 commit、tag 格式，以及**三处**版本一致性：`package.json`、`src-tauri/tauri.conf.json`、**`src-tauri/Cargo.toml`**。第三处不能漏：Cargo 的 `version` 是 `CARGO_PKG_VERSION` 的来源，而 `releases.rs` 把它当对外 User-Agent（`dsh-xlink/<版本>`），漏同步会让外壳向 npm registry / GitHub 自称另一个版本。
2. `quality` 与两个平台的 `build` 在预检通过后并行启动。质量门禁继续执行 UI 测试、UI 构建、bundle 预算、Rust 格式检查、测试和 Clippy；构建矩阵只包含 `macos-15-intel` 与 `windows-latest`，`max-parallel: 2`。
3. 平台 `build` 只执行签名构建并上传 Actions artifact，不创建或修改 GitHub Release。这样质量检查不会留下半成品，两个平台也不再争用同一份 `latest.json`。
4. `publish` 只在预检、质量门禁和两个平台构建全部成功后运行。它下载两个 artifact，校验五个安装/更新文件各有且只有一份，由 `scripts/generate-updater-manifest.mjs` 生成 `latest.json`，再一次性上传全部资产。
5. 新 Release 先以 draft 形式接收完整资产，上传结束后才转换为正式 release；最终 `draft=false`、`prerelease=false`，因此 `releases/latest/download/latest.json` 在发布完成时才对用户可见。
6. 端到端校验分两段：**draft 阶段**（`Normalize draft assets before publishing`，在转正之前）读回 Release 的真实资产 id 与 name，删除计划外资产并断言恰好 6 个且逐名吻合；**转正之后**（`Verify published release`）再拉取更新端点，断言它报告的版本就是本次发布的版本、且每个平台资产都能以 HTTP 200 下载。后者才是「用户到底能不能收到更新」的唯一端到端检查——资产数量断言放在转正前，是为了在 Release 还是 draft 时就能发现上传漏项而不必先把它公开。

`publish` 要求 tag 已存在且与发布 commit 一致，仅复用对应的 draft Release，正式 Release 会直接拒绝覆盖。版本文件未同步、tag 缺失 / 不匹配 / 不在 `main`，或任一构建产物缺失都会在发布前失败。构建用 `TAURI_SIGNING_PRIVATE_KEY` 给更新制品签名。

## 并发与版本单调性

- **发布串行化**：workflow 使用固定的 `concurrency` 组（`group: desktop-release`），因此 tag push 与手动 dispatch 不会并行跑两条流水线。以 `github.ref` 分组时二者属于不同组，同一版本可以并发构建，后启动的那条会在 `Create or reuse release` 步骤上撞车失败——白烧一次双平台构建。
- **版本单调性守卫**：`preflight` 会拉取线上 `releases/latest/download/latest.json`，断言待发布版本**严格大于**它报告的版本，否则直接失败。原因是 GitHub 的 `releases/latest` 取「最近创建的非 draft 非 prerelease release」，而不是 semver 最大值：给旧线发一个 hotfix 会让端点后退，而 Tauri 只在 `release.version > current_version` 时提示更新，已经装上新版本的用户从此静默收不到任何更新。线上还没有可读的 `latest.json`（首次发布）时该检查自动跳过。
- **供应链**：workflow 中所有 `uses:` 都固定到 40 位 commit SHA（后缀注释是对应版本，由 Dependabot 负责升级）。build job 会把更新签名私钥放进构建步骤的环境，一个被投毒或被重指 tag 的第三方 action 足以读走私钥，从而为任意载荷签名。升级路径写在 `.github/dependabot.yml`（github-actions / npm / cargo 三处，次要与补丁版本合并成 PR，主版本单独成 PR）；固定的 SHA 不会被任何东西自动推进，因此 `scripts/check-invariants.mjs` 会同时校验「全部 `uses:` 固定到 SHA」与「`.github/dependabot.yml` 存在且覆盖 github-actions」——少了后者，这条策略就只是文字。

## 缓存与构建

- `setup-node` 为 pnpm store 建立缓存，依赖使用 `pnpm install --frozen-lockfile --prefer-offline`。
- `Swatinem/rust-cache@v2` 分别缓存质量 job 与 release job 的 Cargo registry、Git 依赖和编译结果；缓存 key 含 runner 系统与架构，避免 debug 检查产物污染 release target。缓存只在 `main` 分支保存：GitHub 对不同 tag 使用独立的缓存作用域，tag 之间无法复用彼此的缓存，继续保存这些缓存只会延长 job 收尾时间。
- `src-tauri/Cargo.toml` 使用 `lto = "thin"` 与 `codegen-units = 16`。这比完整 LTO 与单 codegen unit 更适合 CI 冷构建，同时保留 release 优化。
- artifact 使用零压缩上传。DMG、tar.gz 和安装包本身已经压缩，减少 runner 上的重复压缩时间。
- 新增 `bundle.resources` 目录时确认全新 checkout 能生成；所有会执行 Cargo 的 job 先运行资源生成步骤（内置插件用 `npm run prep:builtin`）。接线规则由 `scripts/dev-builtin.test.mjs` 检查，不能依赖本机已有资源。细节见[开发验证](development.md#门禁与反向验证)。

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

## 资产维护

最终资产共 6 个：Windows 安装包及签名、Intel macOS DMG、macOS updater 包及签名、`latest.json`。先有正确 tag 再创建 Release，避免 `untagged-*`。

操作草稿资产使用 Release ID，不依赖 tag 查找或上传。API 路径：

| 操作 | 路径 |
| --- | --- |
| 列表 | `GET /repos/{owner}/{repo}/releases/{release_id}/assets` |
| 删除 | `DELETE /repos/{owner}/{repo}/releases/assets/{asset_id}` |
| 上传 | `https://uploads.github.com/repos/{owner}/{repo}/releases/{release_id}/assets` |

上传使用 `Content-Type: application/octet-stream` 和文件输入，不发到普通 `api.github.com` 上传地址。

## 排障与发布后验证

跟踪状态用 `gh run watch <run-id> --exit-status --interval 15 --compact`，不自写轮询循环。失败后先取具体 job 日志：

```sh
gh run view <run-id> --job <job-id> --log-failed
```

- 质量 job 失败时，构建 job 可能已经完成，但 `publish` 会保持跳过，不会创建正式 Release。
- 如果构建变慢，先确认发布是否从 `main` 手动 dispatch，再查看 `Cache Rust release dependencies` 与 `Install JavaScript dependencies` 的命中情况，区分缓存冷启动、依赖编译和 runner 排队耗时。
- 如果 `latest.json` 生成失败，先检查 artifact 中是否只有一份 `.dmg`、`.app.tar.gz`、`.app.tar.gz.sig`、`*-setup.exe` 与 `*-setup.exe.sig`，以及各文件名是否带当前版本。
- 如果 updater 返回 404，确认最终 Release 是正式 release，且 `latest.json` 已与五个资产一起上传；不要把 `draft` 或 `prerelease` 留为 true。

完成后核对 Release、tag 和 updater 端点，确认版本与资产一致：

```sh
gh api repos/July-X/dsh-xlink/releases/tags/desktop-v<version>
gh api repos/July-X/dsh-xlink/git/ref/tags/desktop-v<version>
curl -fsSL https://github.com/July-X/dsh-xlink/releases/latest/download/latest.json
```
