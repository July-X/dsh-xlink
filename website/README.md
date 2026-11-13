# dsh-xlink 宣传站

静态站点，直接打开 `index.html`，或在仓库根目录运行：

```sh
python3 -m http.server 8080 --directory website
```

然后访问 <http://localhost:8080>。`features.html` 是功能说明页；站点不依赖构建工具或外部资源。

界面图基于 v0.7.6 的管理面板源码渲染（窗口 1040×748、2× 缩放），使用模拟的桌面桥响应和示例数据，不包含用户数据，也不作为实机功能验收证据。首页两张概览图与功能页的 MiniMax 内核图使用同一组示例数据，两种主题的内容保持一致。更新截图时应使用同一窗口尺寸和同一组示例数据。功能描述以仓库当前实现和对应专题的验收状态为准。

检查主题对比交互：`node --test website/site.test.mjs`。

## GitHub Pages 部署

部署地址：<https://july-x.github.io/dsh-xlink/>，首次部署成功后可访问。仓库的 Pages 发布源选择 GitHub Actions；`.github/workflows/website-pages.yml` 在 `main` 分支的 `website/` 或工作流文件变更时运行测试并发布 `website/`，也可以手动运行。只有网站目录会作为 Pages 制品上传，目录内的文件均可公开访问。

## Cloudflare Pages 部署

在线地址：<https://dsh-xlink.pages.dev/>。Pages 连接 `July-X/dsh-xlink` 的 `main` 分支，推送后自动部署。配置为仓库根目录、构建命令 `exit 0`、构建输出目录 `website`；构建变量 `SKIP_DEPENDENCY_INSTALL=1` 避免安装桌面应用的依赖。

Cloudflare 的构建环境可以读取仓库，GitHub 应用权限覆盖授权的整个仓库；网站对外发布的是 `website/` 下的文件。Pages 默认也会公开预览部署，不要把密钥等私密内容放入网站目录。
