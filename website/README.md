# dsh-xlink 宣传站

静态站点，直接打开 `index.html`，或在仓库根目录运行：

```sh
python3 -m http.server 8080 --directory website
```

然后访问 <http://localhost:8080>。`features.html` 是功能说明页；站点不依赖构建工具或外部资源。

首页两张概览图基于 v0.7.5 的管理面板源码渲染，使用模拟的桌面桥响应和示例数据，不包含用户数据，也不作为实机功能验收证据。更新截图时应使用同一窗口尺寸和同一组示例数据，保持两种主题内容对齐。功能描述以仓库当前实现和对应专题的验收状态为准。

检查主题对比交互：`node --test website/site.test.mjs`。
