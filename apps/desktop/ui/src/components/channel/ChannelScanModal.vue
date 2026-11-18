<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { NButton, NInputNumber, NSelect } from 'naive-ui'
import AppIcon from '@/components/AppIcon.vue'
import ModalShell from '@/components/ModalShell.vue'
import { useProjectStore } from '@/stores/project'
import { errorMessage } from '@/stores/toast'
import { addressLabel, channelSummary } from '@/utils/transport'
import type { ChannelDto, DeviceAddress } from '@/api/types'

// 在一条链路上逐个地址探测设备；扫到的地址可直接落成设备。
const props = defineProps<{ channel: ChannelDto }>()
const emit = defineEmits<{ close: []; pick: [address: DeviceAddress, packageId?: string] }>()

const project = useProjectStore()

const from = ref<number | null>(1)
const to = ref<number | null>(32)
const choice = ref<string | null>(null)
const scanning = ref(false)
const found = ref<DeviceAddress[]>([])
const error = ref('')
const options = computed(() => project.drivers.flatMap((driver) => {
  if (!driver.transports.includes(props.channel.transport)) return []
  const bounds = driver.scanBounds ?? (driver.id === 'modbus' ? { from: 1, to: 247 } : null)
  if (!bounds) return []
  const profiles = project.packages.filter((pkg) => pkg.protocol.driver === driver.id &&
    (!pkg.protocol.transports.length || pkg.protocol.transports.includes(props.channel.transport)))
  return [
    ...(!driver.scanRequiresPackage ? [{ label: driver.name, value: driver.id, driver: driver.id, packageId: undefined, bounds }] : []),
    ...profiles.map((pkg) => ({ label: `${pkg.name} · ${driver.name}`, value: pkg.id, driver: driver.id, packageId: pkg.id, bounds })),
  ]
}))
const selected = computed(() => options.value.find((option) => option.value === choice.value))
watch(options, (items) => {
  if (items.some(({ value }) => choice.value === value)) return
  const existing = project.devices.find((device) => device.channelId === props.channel.id)?.package
  choice.value = items.find(({ packageId }) => packageId && packageId === existing)?.value ?? items[0]?.value ?? null
}, { immediate: true })
watch(selected, (next, previous) => {
  if (next?.value === previous?.value) return
  from.value = next?.bounds.from ?? null
  to.value = next ? Math.min(next.bounds.from + 31, next.bounds.to) : null
  found.value = []
  error.value = ''
}, { immediate: true })

async function run(): Promise<void> {
  const mode = selected.value
  const start = from.value
  const end = to.value
  if (!mode || start === null || end === null || start < mode.bounds.from || end > mode.bounds.to || start > end) {
    error.value = '请选择探测协议并填写有效的扫描范围'
    return
  }
  scanning.value = true
  found.value = []
  error.value = ''
  try {
    found.value = await project.scanChannel(
      props.channel.id,
      { from: start, to: end },
      mode.driver,
      mode.packageId,
    )
    if (found.value.length === 0) {
      error.value = '当前范围内没有设备应答。请核对通道参数、设备地址和探测协议。'
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
    <label class="field"><span>探测协议或设备包</span>
      <n-select v-model:value="choice" :options="options" :disabled="scanning" placeholder="此通道暂无可用探测协议" />
    </label>
    <div class="scan-controls mt-12">
      <div class="field">
        <label>起始地址</label>
        <n-input-number v-model:value="from" :min="selected?.bounds.from" :max="selected?.bounds.to" :precision="0" :disabled="scanning" class="w-120" />
      </div>
      <div class="field">
        <label>结束地址</label>
        <n-input-number v-model:value="to" :min="selected?.bounds.from" :max="selected?.bounds.to" :precision="0" :disabled="scanning" class="w-120" />
      </div>
      <div class="spacer" />
      <n-button type="primary" size="small" :loading="scanning" :disabled="scanning || !selected" @click="run">
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
          <n-button size="tiny" type="primary" quaternary @click="emit('pick', s, selected?.packageId)">添加设备</n-button>
        </div>
      </div>
    </div>
    <template #footer>
      <n-button size="small" @click="emit('close')">关闭</n-button>
    </template>
  </ModalShell>
</template>
