/** 附件引用 → Responses 图片片段。读取与校验归内核附件服务，插件不读任意路径。 */
const MAX_PIXELS = 4 * 1024 * 1024;
const MAX_IMAGE_BYTES = 1024 * 1024;
export const MAX_REQUEST_IMAGE_BYTES = 16 * 1024 * 1024;

export async function prepareImageParts(messages, services, signal) {
  const parts = new Map();
  const versions = new Map();
  const blocks = messages.flatMap((message) => message.content ?? [])
    .filter((block) => block.type === 'image');
  if (blocks.length === 0) return parts;
  const { attachments, handleText, offloadedText, resolveAccess, requiredOffload } = services;
  if (attachments === undefined) {
    throw Object.assign(new Error('内核附件服务不可用，无法读取图片；请重启工作台后重试'), { code: 'ATTACHMENT_UNAVAILABLE' });
  }
  for (const block of blocks) {
    signal?.throwIfAborted();
    const ref = block.attachment;
    const access = resolveAccess(ref);
    if (block.offloaded === true) {
      parts.set(block, [{ type: 'input_text', text: offloadedText(ref, access) }]);
      continue;
    }
    let version = versions.get(ref.attachmentId);
    if (version === undefined) {
      const scale = Math.min(1, Math.sqrt(MAX_PIXELS / (ref.width * ref.height)));
      version = await attachments.readImageRequest(ref, {
        width: Math.max(1, Math.floor(ref.width * scale)),
        height: Math.max(1, Math.floor(ref.height * scale)),
        maxBytes: MAX_IMAGE_BYTES,
      }, signal);
      signal?.throwIfAborted();
      if (!(version.data instanceof Uint8Array) || version.data.length === 0
          || !['image/png', 'image/jpeg', 'image/webp', 'image/gif'].includes(version.mediaType)) {
        throw Object.assign(new Error('内核返回了无效的请求图片；请重新添加图片后重试'), { code: 'INVALID_IMAGE' });
      }
      versions.set(ref.attachmentId, version);
    }
    parts.set(block, [
      { type: 'input_text', text: handleText(ref, version, access) },
      { type: 'input_image', image_url: `data:${version.mediaType};base64,${Buffer.from(version.data).toString('base64')}`, detail: 'auto' },
    ]);
  }
  const offloadImages = requiredOffload(messages, {
    representation: 'base64', maxBytes: MAX_REQUEST_IMAGE_BYTES,
  }, (block) => versions.get(block.attachment.attachmentId).data.length);
  if (offloadImages > 0) {
    throw Object.assign(new Error(`图片请求超过 16 MiB，需要卸载 ${offloadImages} 张最早的历史图片后重试`), {
      code: 'IMAGE_OFFLOAD_REQUIRED', offloadImages,
    });
  }
  return parts;
}
