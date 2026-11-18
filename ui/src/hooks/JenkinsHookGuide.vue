<script setup>
import { computed, ref } from 'vue';
import scriptSource from '../../../scripts/jenkins-hook-postbuild.groovy?raw';
import { jenkinsScript } from './jenkinsGuide.js';
import { copyText } from './hooks.js';

const props = defineProps({ address: { type: String, default: '' } });
const credentialsId = ref('dsh-hook-token');
const message = ref('构建通知');
const files = ref('');
const log = ref('on_failure');
const prepared = computed(() => {
  try { return { script: jenkinsScript(scriptSource, { url: props.address, credentialsId: credentialsId.value, message: message.value, files: files.value, log: log.value }), error: '' }; }
  catch (error) { return { script: '', error: error.message }; }
});
function copyScript() {
  if (prepared.value.script) copyText(prepared.value.script, '完整 Groovy 脚本（不含令牌）');
}
</script>

<template>
  <div class="jenkins-guide">
    <ol class="jenkins-guide__steps">
      <li>管理员安装兼容的 <strong>Groovy Postbuild</strong> 插件，无需 Node.js。把此接入点的 <strong>Bearer 令牌</strong>存入 Jenkins 的 Secret text 凭据，ID 填在下方。不要使用旧 Notification 密码。</li>
      <li>Freestyle「构建后操作 → Groovy Postbuild」粘贴完整脚本，放在结果处理之后、workspace 清理之前；管理员完成 <strong>Script Approval</strong>，不要关闭全局脚本安全。</li>
      <li>执行一次构建，在本页收件箱检查消息和附件。请求从 <strong>Controller</strong> 发出，请放行其来源网段。</li>
    </ol>
    <el-form label-width="100px" size="small">
      <el-form-item label="接入点 URL"><el-input :model-value="address" readonly placeholder="先在设置中启用可信网络" /></el-form-item>
      <el-form-item label="凭据 ID"><el-input v-model="credentialsId" placeholder="Secret text 的 ID，不是令牌" /></el-form-item>
      <el-form-item label="消息"><el-input v-model="message" placeholder="脚本自动附任务、构建号与状态" /></el-form-item>
      <el-form-item label="日志策略">
        <el-select v-model="log" aria-label="Jenkins 日志策略">
          <el-option value="on_failure" label="仅异常状态（默认）" />
          <el-option value="always" label="每次构建" />
          <el-option value="off" label="不附日志" />
        </el-select>
      </el-form-item>
      <el-form-item label="文件（可选）"><el-input v-model="files" type="textarea" :rows="2" placeholder="workspace 相对路径，每行一个，如 reports/result.txt" /></el-form-item>
    </el-form>
    <p class="jenkins-guide__note">状态与日志是 Postbuild 快照，不是严格最终结果；日志只保留开头最多 1 MiB，末尾失败信息可能不在其中，请通过构建链接查看完整控制台。文件支持 .txt/.log、PNG/JPEG/WebP，不支持压缩包。发送失败不改构建结果，也不保证离线补发。</p>
    <details>
      <summary>凭据与运行边界</summary>
      <p class="jenkins-guide__note">这里只配置凭据 ID，复制内容不含令牌；URL 和 ID 仅限可信管理员维护，不能开放为任意构建参数。HTTP 为明文，仅限可信内网。网络投递共用 30 秒预算，连接 3 秒、读取空闲 5 秒，每个请求最多 2 次尝试；预算不含 Agent 文件和日志读取，不保证 Job 在 30 秒内结束。大附件或慢网络可能超时。修改脚本配置可能需要重新审批；插件自身的失败行为也应设为不影响构建结果。日志上传前请确认已脱敏。</p>
    </details>
    <p v-if="prepared.error" class="jenkins-guide__note" role="status">{{ prepared.error }}</p>
    <div class="dialog-actions"><el-button type="primary" :disabled="!prepared.script" @click="copyScript">复制完整 Groovy 脚本</el-button></div>
  </div>
</template>

<style scoped>
.jenkins-guide { display: flex; flex-direction: column; gap: 12px; min-width: 0; }
.jenkins-guide__steps { margin: 0; padding-left: 22px; line-height: 1.6; }
.jenkins-guide__steps li + li { margin-top: 6px; }
.jenkins-guide__note { margin: 0; color: var(--text-secondary); font-size: var(--fs-3); line-height: 1.6; overflow-wrap: anywhere; }
.jenkins-guide :deep(.el-form-item:last-child) { margin-bottom: 0; }
</style>
