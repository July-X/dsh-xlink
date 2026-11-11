// 端口草稿：外部轮询快照只回填**没有草稿**的字段。
//
// 焦点不是「放弃修改」的信号——用户点了卡片空白处之后，下一轮 get_status 照样会
// 把没保存的输入覆盖回旧值。原实现用 focusin/focusout 保护整张表单，还导致
// 「首个快照在聚焦期间到达就永远不回填」。所以判据是逐字段的 dirty。
//
// `field` / `dirty` 是 Vue ref。即使快照恰好等于草稿，也不能证明它确认了
// 最新编辑（用户可能改动后又改回）；只有对应提交成功才能解除草稿。
export function adoptDraft(field, dirty, incoming) {
  if (!dirty.value) field.value = incoming;
}
