# dsh-xlink 宣传站

静态站点，直接打开 `index.html`，或在仓库根目录运行：

```sh
python3 -m http.server 8080 --directory website
```

然后访问 <http://localhost:8080>。`features.html` 是功能说明页；站点不依赖构建工具或外部资源。

七张界面图基于 `ccb6250f8cc6f35ac9611d45441b15788d8c3329` 的真实 Vue 管理面板渲染：概览、插件和记忆各含明暗两种主题，另有 MiniMax 深色概览。窗口为 1040×748 CSS 像素、2× 缩放；使用公开虚构的桌面桥响应和示例数据，不读取用户数据。截图由 Windows Edge 模拟 macOS 外观生成，不是 macOS 原生截图或后端功能验收。两种主题使用同一组示例数据。复现步骤见 [截图脚本说明](../scripts/website-preview/README.md)，前后对比见 [网站改动记录](../docs/website-preview/README.md)。功能描述以当前源码与对应专题验收状态为准，未发布的 main 改动可能与下载包不同。

检查主题对比交互：`node --test website/site.test.mjs`。

## GitHub Pages 部署

部署地址：<https://july-x.github.io/dsh-xlink/>，首次部署成功后可访问。仓库的 Pages 发布源选择 GitHub Actions；`.github/workflows/website-pages.yml` 在 `main` 分支的 `website/` 或工作流文件变更时运行测试并发布 `website/`，也可以手动运行。只有网站目录会作为 Pages 制品上传，目录内的文件均可公开访问。

## Cloudflare Pages 部署

在线地址：<https://dsh-xlink.pages.dev/>。Pages 连接 `July-X/dsh-xlink` 的 `main` 分支，推送后自动部署。配置为仓库根目录、构建命令 `exit 0`、构建输出目录 `website`；构建变量 `SKIP_DEPENDENCY_INSTALL=1` 避免安装桌面应用的依赖。

Cloudflare 的构建环境可以读取仓库，GitHub 应用权限覆盖授权的整个仓库；网站对外发布的是 `website/` 下的文件。Pages 默认也会公开预览部署，不要把密钥等私密内容放入网站目录。
