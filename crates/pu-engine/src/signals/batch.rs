//! Engine-owned, bounded single-flight native source sampling.
use std::collections::HashMap;
use std::future::Future;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};
use std::time::Duration;
use tokio::time::Instant;

const MAX_ROOTS: usize = 256;

pub(super) struct Sampler<K, V> {
    roots: Mutex<HashMap<String, (u64, K, usize)>>,
    next: AtomicU64,
    cache: tokio::sync::Mutex<Cache<V>>,
    period: Duration,
}
struct Cache<V> {
    started: Option<Instant>,
    completed: Option<Instant>,
    values: HashMap<String, (u64, Option<V>)>,
}
pub(super) struct Registration<K, V> {
    sampler: Arc<Sampler<K, V>>,
    id: String,
    epoch: u64,
}
impl<K, V> Drop for Registration<K, V> {
    fn drop(&mut self) {
        let mut roots = self.sampler.roots.lock().unwrap();
        let remove = roots
            .get_mut(&self.id)
            .is_some_and(|(epoch, _, references)| {
                if *epoch != self.epoch {
                    return false;
                }
                *references -= 1;
                *references == 0
            });
        if remove {
            roots.remove(&self.id);
        }
    }
}
impl<K: Clone, V: Clone> Sampler<K, V> {
    pub(super) fn new(period: Duration) -> Self {
        Self {
            roots: Mutex::new(HashMap::new()),
            next: AtomicU64::new(0),
            cache: tokio::sync::Mutex::new(Cache {
                started: None,
                completed: None,
                values: HashMap::new(),
            }),
            period,
        }
    }
    pub(super) fn register(self: &Arc<Self>, id: String, key: K) -> Option<Registration<K, V>> {
        let mut roots = self.roots.lock().unwrap();
        if roots.len() >= MAX_ROOTS || roots.contains_key(&id) {
            return None;
        }
        let epoch = self
            .next
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .ok()?;
        roots.insert(id.clone(), (epoch, key, 1));
        Some(Registration {
            sampler: self.clone(),
            id,
            epoch,
        })
    }
    /// Share an existing registration only for its exact owned-source key.
    pub(super) fn attach(self: &Arc<Self>, id: &str, key: &K) -> Option<Registration<K, V>>
    where
        K: PartialEq,
    {
        let mut roots = self.roots.lock().unwrap();
        let (epoch, existing, references) = roots.get_mut(id)?;
        if existing != key {
            return None;
        }
        *references = references.checked_add(1)?;
        Some(Registration {
            sampler: self.clone(),
            id: id.into(),
            epoch: *epoch,
        })
    }
    // Callers share one bounded refresh. A native edge can request a newer view,
    // but simultaneous edges coalesce and never trigger more than four scans/s.
    // Cached source timestamps are returned unchanged, never renewed on receipt.
    pub(super) async fn sample<F, Fut>(&self, id: &str, urgent: bool, refresh: F) -> Option<V>
    where
        F: FnOnce(Vec<(String, K)>) -> Fut,
        Fut: Future<Output = HashMap<String, Option<V>>>,
    {
        let requested = Instant::now();
        let epoch = self.roots.lock().unwrap().get(id)?.0;
        let mut cache = self.cache.lock().await;
        let needs_refresh = cache
            .values
            .get(id)
            .is_none_or(|(cached, _)| *cached != epoch)
            || cache.completed.is_none_or(|at| {
                if urgent {
                    at <= requested
                } else {
                    cache.started.is_none_or(|at| at.elapsed() >= self.period)
                }
            });
        if needs_refresh {
            if let Some(at) = cache.completed {
                tokio::time::sleep_until(at + Duration::from_millis(250)).await;
            }
            let registered: Vec<_> = self
                .roots
                .lock()
                .unwrap()
                .iter()
                .map(|(id, (epoch, key, _))| (id.clone(), (*epoch, key.clone())))
                .collect();
            let roots = registered
                .iter()
                .map(|(id, (_, key))| (id.clone(), key.clone()))
                .collect();
            let started = Instant::now();
            let mut values = refresh(roots).await;
            cache.started = Some(started);
            // Failure is cached too; an unavailable listener must not create an
            // unbounded retry storm. Never accept values for unregistered roots.
            cache.values = registered
                .into_iter()
                .map(|(id, (epoch, _))| {
                    let value = values.remove(&id).flatten();
                    (id, (epoch, value))
                })
                .collect();
            cache.completed = Some(Instant::now());
        }
        if self
            .roots
            .lock()
            .unwrap()
            .get(id)
            .is_none_or(|(current, _, _)| *current != epoch)
        {
            return None;
        }
        cache
            .values
            .get(id)
            .filter(|(cached, _)| *cached == epoch)
            .and_then(|(_, value)| value.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test(start_paused = true)]
    async fn given_binding_reader_and_observer_should_share_registration_without_borrowing_another_key()
     {
        let sampler = Arc::new(Sampler::<usize, usize>::new(Duration::from_secs(2)));
        let observer = sampler.register("launch".into(), 7).unwrap();
        let reader = sampler.attach("launch", &7).unwrap();
        assert!(sampler.attach("launch", &8).is_none());
        assert_eq!(sampler.roots.lock().unwrap().len(), 1);
        drop(observer);
        assert_eq!(
            sampler
                .sample("launch", false, |roots| async {
                    roots.into_iter().map(|(id, key)| (id, Some(key))).collect()
                })
                .await,
            Some(7)
        );
        drop(reader);
        assert_eq!(sampler.roots.lock().unwrap().len(), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn given_failed_source_should_cache_failure_and_bound_urgent_edge_retries() {
        let sampler = Arc::new(Sampler::<usize, usize>::new(Duration::from_secs(2)));
        let _guard = sampler.register("launch".into(), 1).unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        for _ in 0..100 {
            assert_eq!(
                sampler
                    .sample("launch", false, |_| {
                        calls.fetch_add(1, Ordering::SeqCst);
                        async { HashMap::new() }
                    })
                    .await,
                None
            );
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let began = Instant::now();
        let mut tasks = Vec::new();
        for _ in 0..100 {
            let (sampler, calls) = (sampler.clone(), calls.clone());
            tasks.push(tokio::spawn(async move {
                sampler
                    .sample("launch", true, |_| async {
                        calls.fetch_add(1, Ordering::SeqCst);
                        tokio::time::sleep(Duration::from_millis(1)).await;
                        HashMap::from([("launch".into(), Some(7))])
                    })
                    .await
            }));
        }
        for task in tasks {
            assert_eq!(task.await.unwrap(), Some(7));
        }
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert!(began.elapsed() >= Duration::from_millis(250));
    }

    #[tokio::test(start_paused = true)]
    async fn given_replaced_registration_should_not_reuse_removed_launch_cache() {
        let sampler = Arc::new(Sampler::<usize, usize>::new(Duration::from_secs(2)));
        let first = sampler.register("launch".into(), 1).unwrap();
        let refresh = |roots: Vec<(String, usize)>| async move {
            roots.into_iter().map(|(id, n)| (id, Some(n))).collect()
        };
        assert_eq!(sampler.sample("launch", false, refresh).await, Some(1));
        drop(first);
        let _second = sampler.register("launch".into(), 2).unwrap();
        assert_eq!(sampler.sample("launch", false, refresh).await, Some(2));
    }

    #[tokio::test(start_paused = true)]
    async fn given_many_launches_should_sample_once_and_keep_results_scoped() {
        let sampler = Arc::new(Sampler::<usize, usize>::new(Duration::from_secs(2)));
        let guards: Vec<_> = (0..100)
            .map(|n| sampler.register(n.to_string(), n).unwrap())
            .collect();
        let calls = Arc::new(AtomicUsize::new(0));
        let mut tasks = Vec::new();
        for n in 0..100 {
            let sampler = sampler.clone();
            let calls = calls.clone();
            tasks.push(tokio::spawn(async move {
                let value = sampler
                    .sample(&n.to_string(), false, |roots| async move {
                        calls.fetch_add(1, Ordering::SeqCst);
                        tokio::time::sleep(Duration::from_millis(1)).await;
                        roots.into_iter().map(|(id, n)| (id, Some(n * 2))).collect()
                    })
                    .await;
                assert_eq!(value, Some(n * 2));
            }));
        }
        for task in tasks {
            task.await.unwrap();
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        drop(guards);
        assert_eq!(sampler.roots.lock().unwrap().len(), 0);
        assert_eq!(
            sampler
                .sample("0", false, |_| async { panic!("unregistered launch") })
                .await,
            None
        );
    }
}
