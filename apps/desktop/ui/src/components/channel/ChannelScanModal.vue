<script setup lang="ts">
import { ref } from 'vue'
import { NButton, NInputNumber } from 'naive-ui'
import AppIcon from '@/components/AppIcon.vue'
import ModalShell from '@/components/ModalShell.vue'
import { useProjectStore } from '@/stores/project'
import { errorMessage } from '@/stores/toast'
import { addressLabel, channelSummary } from '@/utils/transport'
import type { ChannelDto, DeviceAddress } from '@/api/types'

// 在一条链路上逐个地址探测设备；扫到的地址可直接落成设备。
const props = defineProps<{ channel: ChannelDto }>()
const emit = defineEmits<{ close: []; pick: [address: DeviceAddress] }>()

const project = useProjectStore()

const from = ref(1)
const to = ref(32)
const scanning = ref(false)
const found = ref<DeviceAddress[]>([])
const error = ref('')

async function run(): Promise<void> {
  if (from.value < 1 || to.value > 247 || from.value > to.value) {
    error.value = '地址范围需在 1–247 且起始不大于结束'
    return
  }
  scanning.value = true
  found.value = []
  error.value = ''
  try {
    found.value = await project.scanChannel(
      props.channel.id,
      { from: from.value, to: to.value },
      null,
    )
    if (found.value.length === 0) {
      error.value = '未发现设备：检查接线、波特率/校验、地址范围；若通道混用多种驱动，请先在设备页指定'
    }
  } catch (e) {
    error.value = errorMessage(e)
  } finally {
    scanning.value = false
  }
}
</script>

<template>
  <ModalShell
    title="扫描设备地址"
    :subtitle="`${channel.name || channel.id}（${channelSummary(channel)}）· 探测与轮询串行执行，请等待`"
    :width="560"
    @close="emit('close')"
  >
    <div class="scan-controls">
      <div class="field">
        <label>起始地址</label>
        <n-input-number v-model:value="from" :min="1" :max="247" style="width: 120px" />
      </div>
      <div class="field">
        <label>结束地址</label>
        <n-input-number v-model:value="to" :min="1" :max="247" style="width: 120px" />
      </div>
      <div class="spacer" />
      <n-button type="primary" size="small" :loading="scanning" :disabled="scanning" @click="run">
        <template #icon><AppIcon name="scan" :size="15" /></template>
        {{ scanning ? '扫描中…' : '开始扫描' }}
      </n-button>
    </div>
    <div v-if="error" class="banner warn mt-12">
      <AppIcon name="alert" :size="16" />
      <span>{{ error }}</span>
    </div>
    <div v-if="found.length" class="scan-result">
      <div class="muted small">发现 {{ found.length }} 个设备</div>
      <div class="scan-chips">
        <div v-for="s in found" :key="String(s)" class="scan-chip">
          <span class="mono">{{ addressLabel(s) }}</span>
          <n-button size="tiny" type="primary" quaternary @click="emit('pick', s)">添加设备</n-button>
        </div>
      </div>
    </div>
    <p class="hint mt-12">
      扫描会逐个地址探测，1–32 约需十几秒。若全部无响应，优先核对波特率、校验位与 A/B 线序。
    </p>
    <template #footer>
      <n-button size="small" @click="emit('close')">关闭</n-button>
    </template>
  </ModalShell>
</template>
