//! TagHub：实时库 —— 最新值缓存 + 按订阅集过滤的增量分发。

use dashmap::DashMap;
use hmi_domain::value::{now_ms, Quality, TagChange, TagId, TagSnapshot, Value};
use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;
use tokio::sync::mpsc;

/// 实时库。
///
/// - 写侧：通道监督器按事务批量写入（值 + 质量 + 时间戳）；
/// - 读侧：缓存随时可查（`get` / `snapshot_all`）；
/// - 订阅：每个订阅一个过滤集 + 有界通道；变更仅推给订阅了该 tag 的订阅者。
pub struct TagHub {
    cache: DashMap<TagId, TagSnapshot>,
    subs: DashMap<u64, Sub>,
    next_id: AtomicU64,
    dropped: AtomicU64,
}

struct Sub {
    filter: RwLock<HashSet<TagId>>,
    tx: mpsc::Sender<Vec<TagChange>>,
}

/// 订阅句柄。drop 后自动失效（也可显式 `hub.unsubscribe(id)`）。
pub struct Subscription {
    pub id: u64,
    pub rx: mpsc::Receiver<Vec<TagChange>>,
}

impl TagHub {
    pub fn new() -> Self {
        Self {
            cache: DashMap::new(),
            subs: DashMap::new(),
            next_id: AtomicU64::new(1),
            dropped: AtomicU64::new(0),
        }
    }

    /// 批量写入。同一 tag 在批内出现多次时只保留最后一个。
    pub fn write(&self, batch: Vec<(TagId, TagSnapshot)>) {
        if batch.is_empty() {
            return;
        }
        let mut changes: Vec<TagChange> = Vec::with_capacity(batch.len());
        for (id, snap) in batch {
            self.cache.insert(id.clone(), snap.clone());
            // 批内去重：同 tag 保留最后一次
            if let Some(pos) = changes.iter().position(|c| c.tag == id) {
                changes[pos] = TagChange {
                    tag: id,
                    value: snap.value,
                    quality: snap.quality,
                    ts_ms: snap.ts_ms,
                };
            } else {
                changes.push((id.clone(), &snap).into());
            }
        }

        for entry in self.subs.iter() {
            let filter = entry.value().filter.read().unwrap();
            let hits: Vec<TagChange> = changes
                .iter()
                .filter(|c| filter.contains(&c.tag))
                .cloned()
                .collect();
            if hits.is_empty() {
                continue;
            }
            // 满了就丢（慢消费者不该拖垮采集）；计数供诊断
            if entry.value().tx.try_send(hits).is_err() {
                let n = self.dropped.fetch_add(1, Ordering::Relaxed);
                if n % 1000 == 0 {
                    tracing::warn!(dropped_batches = n, "订阅者消费过慢，已丢弃变更批次");
                }
            }
        }
    }

    /// 订阅一组 tag。通道缓冲 1024。
    pub fn subscribe(&self, tags: Vec<TagId>) -> Subscription {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel(1024);
        self.subs.insert(
            id,
            Sub {
                filter: RwLock::new(tags.into_iter().collect()),
                tx,
            },
        );
        Subscription { id, rx }
    }

    /// 更新订阅过滤集（增量订阅/退订后的重算）。
    pub fn update_filter(&self, id: u64, tags: Vec<TagId>) {
        if let Some(sub) = self.subs.get(&id) {
            *sub.value().filter.write().unwrap() = tags.into_iter().collect();
        }
    }

    pub fn unsubscribe(&self, id: u64) {
        self.subs.remove(&id);
    }

    pub fn get(&self, id: &TagId) -> Option<TagSnapshot> {
        self.cache.get(id).map(|s| s.value().clone())
    }

    /// 全部当前值（快照用）。
    pub fn snapshot_all(&self) -> Vec<TagChange> {
        self.cache
            .iter()
            .map(|e| (e.key().clone(), e.value()).into())
            .collect()
    }

    /// 指定 tag 的当前值（订阅后的首屏）。
    pub fn snapshot_of(&self, tags: &[TagId]) -> Vec<TagChange> {
        tags.iter()
            .filter_map(|t| self.get(t).map(|s| (t.clone(), &s).into()))
            .collect()
    }

    /// 构造一个 CommLost 批次：保留最后已知值（若无则 0 占位），质量置 CommLost。
    pub fn comm_lost_batch(&self, tags: &[TagId]) -> Vec<(TagId, TagSnapshot)> {
        let ts = now_ms();
        tags.iter()
            .map(|t| {
                let value = self
                    .cache
                    .get(t)
                    .map(|s| s.value.clone())
                    .unwrap_or(Value::F64(0.0));
                (
                    t.clone(),
                    TagSnapshot {
                        value,
                        quality: Quality::CommLost,
                        ts_ms: ts,
                    },
                )
            })
            .collect()
    }

    pub fn stats(&self) -> (usize, usize, u64) {
        (
            self.cache.len(),
            self.subs.len(),
            self.dropped.load(Ordering::Relaxed),
        )
    }
}

impl Default for TagHub {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(v: f64) -> TagSnapshot {
        TagSnapshot {
            value: Value::F64(v),
            quality: Quality::Good,
            ts_ms: now_ms(),
        }
    }

    #[tokio::test]
    async fn subscribe_filters_and_batches() {
        let hub = TagHub::new();
        let t1 = TagId::new("d.a");
        let t2 = TagId::new("d.b");
        let mut sub = hub.subscribe(vec![t1.clone()]);

        hub.write(vec![(t1.clone(), snap(1.0)), (t2.clone(), snap(2.0))]);
        let batch = sub.rx.recv().await.unwrap();
        assert_eq!(batch.len(), 1); // 只收到订阅的 t1
        assert_eq!(batch[0].tag, t1);
        assert_eq!(batch[0].value, Value::F64(1.0));

        // 缓存里有全部
        assert!(hub.get(&t2).is_some());
        assert_eq!(hub.snapshot_all().len(), 2);
    }

    #[tokio::test]
    async fn batch_dedup_keeps_last() {
        let hub = TagHub::new();
        let t = TagId::new("d.a");
        let mut sub = hub.subscribe(vec![t.clone()]);
        hub.write(vec![(t.clone(), snap(1.0)), (t.clone(), snap(9.0))]);
        let batch = sub.rx.recv().await.unwrap();
        assert_eq!(batch.len(), 1);
        assert_eq!(batch[0].value, Value::F64(9.0));
        assert_eq!(hub.get(&t).unwrap().value, Value::F64(9.0));
    }

    #[tokio::test]
    async fn update_filter_changes_delivery() {
        let hub = TagHub::new();
        let t1 = TagId::new("d.a");
        let t2 = TagId::new("d.b");
        let mut sub = hub.subscribe(vec![t1.clone()]);
        hub.update_filter(sub.id, vec![t2.clone()]);

        hub.write(vec![(t1.clone(), snap(1.0)), (t2.clone(), snap(2.0))]);
        let batch = sub.rx.recv().await.unwrap();
        assert_eq!(batch[0].tag, t2);

        hub.unsubscribe(sub.id);
        assert_eq!(hub.stats().1, 0);
    }

    #[test]
    fn snapshot_of_returns_only_known() {
        let hub = TagHub::new();
        let t1 = TagId::new("d.a");
        hub.write(vec![(t1.clone(), snap(5.0))]);
        let t2 = TagId::new("d.b");
        let snaps = hub.snapshot_of(&[t1, t2]);
        assert_eq!(snaps.len(), 1);
    }
}
