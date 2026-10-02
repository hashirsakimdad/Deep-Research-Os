//! Bounded concurrent execution of research tasks with dependency ordering,
//! per-task timeouts, cancellation and partial-failure isolation.

use std::{collections::HashSet, future::Future, time::Duration};

use futures::{StreamExt, stream};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{error::AgentError, models::ResearchTask};

pub struct TaskReport<T> {
    pub task: ResearchTask,
    pub outcome: Result<T, AgentError>,
}

/// Runs `exec` for every task.
///
/// * At most `max_concurrent` tasks run at once (backpressure).
/// * Tasks run in waves: a task starts only after all of its dependencies in
///   this batch succeeded. Dependencies outside the batch count as satisfied.
/// * A task that fails, times out or is cancelled never aborts its siblings;
///   tasks depending on it are reported as failed without running.
pub async fn run_tasks<T, F, Fut>(
    tasks: Vec<ResearchTask>,
    max_concurrent: usize,
    task_timeout: Duration,
    cancel: &CancellationToken,
    exec: F,
) -> Vec<TaskReport<T>>
where
    F: Fn(ResearchTask) -> Fut,
    Fut: Future<Output = Result<T, AgentError>>,
{
    let in_batch: HashSet<Uuid> = tasks.iter().map(|t| t.id).collect();
    let mut succeeded: HashSet<Uuid> = HashSet::new();
    let mut failed: HashSet<Uuid> = HashSet::new();
    let mut pending = tasks;
    let mut reports = Vec::new();

    while !pending.is_empty() {
        let (mut ready, mut waiting): (Vec<_>, Vec<_>) = std::mem::take(&mut pending)
            .into_iter()
            .partition(|t| t.dependencies.iter().all(|d| !in_batch.contains(d) || succeeded.contains(d)));

        // Tasks blocked by a failed dependency are failed without running.
        let (blocked, still_waiting): (Vec<_>, Vec<_>) =
            waiting.drain(..).partition(|t| t.dependencies.iter().any(|d| failed.contains(d)));
        for t in blocked {
            failed.insert(t.id);
            reports.push(TaskReport {
                task: t,
                outcome: Err(AgentError::InvalidInput {
                    agent: "scheduler",
                    reason: "a dependency of this task failed".into(),
                }),
            });
        }
        pending = still_waiting;

        if ready.is_empty() {
            // Remaining tasks wait on each other (cycle) or on unknown ids.
            for t in pending.drain(..) {
                failed.insert(t.id);
                reports.push(TaskReport {
                    task: t,
                    outcome: Err(AgentError::InvalidInput {
                        agent: "scheduler",
                        reason: "unsatisfiable task dependencies".into(),
                    }),
                });
            }
            break;
        }

        // Highest priority first within a wave.
        ready.sort_by_key(|t| match t.priority {
            crate::models::Priority::High => 0,
            crate::models::Priority::Medium => 1,
            crate::models::Priority::Low => 2,
        });

        let exec = &exec;
        let wave: Vec<TaskReport<T>> = stream::iter(ready)
            .map(|task| async move {
                let fut = exec(task.clone());
                let outcome = tokio::select! {
                    _ = cancel.cancelled() => Err(AgentError::Cancelled { agent: "scheduler" }),
                    r = tokio::time::timeout(task_timeout, fut) => match r {
                        Ok(inner) => inner,
                        Err(_) => Err(AgentError::Timeout { agent: "researcher" }),
                    },
                };
                TaskReport { task, outcome }
            })
            .buffer_unordered(max_concurrent.max(1))
            .collect()
            .await;

        for r in &wave {
            if r.outcome.is_ok() {
                succeeded.insert(r.task.id);
            } else {
                failed.insert(r.task.id);
            }
        }
        reports.extend(wave);
    }
    reports
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use chrono::Utc;

    use super::*;
    use crate::models::{Priority, TaskStatus};

    fn task(deps: Vec<Uuid>, priority: Priority) -> ResearchTask {
        ResearchTask {
            id: Uuid::new_v4(),
            research_run_id: Uuid::nil(),
            question_id: None,
            question: "q".into(),
            objective: "o".into(),
            priority,
            status: TaskStatus::Pending,
            assigned_agent: "researcher".into(),
            required_evidence: vec![],
            dependencies: deps,
            iteration: 1,
            error: None,
            created_at: Utc::now(),
            completed_at: None,
        }
    }

    const LONG: Duration = Duration::from_secs(5);

    #[tokio::test]
    async fn never_exceeds_the_concurrency_limit_but_does_run_in_parallel() {
        let tasks: Vec<_> = (0..8).map(|_| task(vec![], Priority::Medium)).collect();
        let (current, peak) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
        let (c, p) = (current.clone(), peak.clone());
        let reports = run_tasks(tasks, 3, LONG, &CancellationToken::new(), move |_| {
            let (c, p) = (c.clone(), p.clone());
            async move {
                let now = c.fetch_add(1, Ordering::SeqCst) + 1;
                p.fetch_max(now, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(20)).await;
                c.fetch_sub(1, Ordering::SeqCst);
                Ok(())
            }
        })
        .await;
        assert_eq!(reports.len(), 8);
        assert!(reports.iter().all(|r| r.outcome.is_ok()));
        assert_eq!(peak.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn one_failure_or_timeout_does_not_sink_the_others() {
        let (ok1, slow, bad, ok2) = (
            task(vec![], Priority::High),
            task(vec![], Priority::Medium),
            task(vec![], Priority::Medium),
            task(vec![], Priority::Low),
        );
        let (slow_id, bad_id) = (slow.id, bad.id);
        let reports = run_tasks(
            vec![ok1, slow, bad, ok2],
            4,
            Duration::from_millis(50),
            &CancellationToken::new(),
            move |t| async move {
                if t.id == slow_id {
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    Ok(())
                } else if t.id == bad_id {
                    Err(AgentError::NoSources { agent: "researcher" })
                } else {
                    Ok(())
                }
            },
        )
        .await;
        let ok = reports.iter().filter(|r| r.outcome.is_ok()).count();
        assert_eq!(ok, 2);
        let slow_report = reports.iter().find(|r| r.task.id == slow_id).unwrap();
        assert!(matches!(slow_report.outcome, Err(AgentError::Timeout { .. })));
        let bad_report = reports.iter().find(|r| r.task.id == bad_id).unwrap();
        assert!(matches!(bad_report.outcome, Err(AgentError::NoSources { .. })));
    }

    #[tokio::test]
    async fn dependencies_run_after_their_prerequisites() {
        let a = task(vec![], Priority::Low);
        let b = task(vec![a.id], Priority::High);
        let c = task(vec![b.id], Priority::High);
        let order = Arc::new(std::sync::Mutex::new(vec![]));
        let (ids, o) = ((a.id, b.id, c.id), order.clone());
        // submit in reverse order to prove ordering comes from dependencies
        let reports = run_tasks(vec![c, b, a], 4, LONG, &CancellationToken::new(), move |t| {
            let o = o.clone();
            async move {
                o.lock().unwrap().push(t.id);
                Ok(())
            }
        })
        .await;
        assert_eq!(reports.len(), 3);
        assert_eq!(*order.lock().unwrap(), vec![ids.0, ids.1, ids.2]);
    }

    #[tokio::test]
    async fn dependents_of_a_failed_task_are_skipped() {
        let a = task(vec![], Priority::High);
        let b = task(vec![a.id], Priority::High);
        let independent = task(vec![], Priority::High);
        let (a_id, b_id) = (a.id, b.id);
        let ran = Arc::new(AtomicUsize::new(0));
        let r = ran.clone();
        let reports = run_tasks(vec![a, b, independent], 4, LONG, &CancellationToken::new(), move |t| {
            let r = r.clone();
            async move {
                r.fetch_add(1, Ordering::SeqCst);
                if t.id == a_id { Err(AgentError::NoSources { agent: "researcher" }) } else { Ok(()) }
            }
        })
        .await;
        assert_eq!(ran.load(Ordering::SeqCst), 2, "dependent never executed");
        assert!(reports.iter().find(|r| r.task.id == b_id).unwrap().outcome.is_err());
        assert_eq!(reports.iter().filter(|r| r.outcome.is_ok()).count(), 1);
    }

    #[tokio::test]
    async fn dependency_cycles_terminate_as_failures() {
        let mut a = task(vec![], Priority::High);
        let mut b = task(vec![], Priority::High);
        a.dependencies = vec![b.id];
        b.dependencies = vec![a.id];
        let reports = run_tasks(vec![a, b], 2, LONG, &CancellationToken::new(), |_| async { Ok(()) }).await;
        assert_eq!(reports.len(), 2);
        assert!(reports.iter().all(|r| r.outcome.is_err()));
    }

    #[tokio::test]
    async fn cancellation_stops_running_tasks() {
        let cancel = CancellationToken::new();
        let c = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(30)).await;
            c.cancel();
        });
        let reports = run_tasks(vec![task(vec![], Priority::High)], 1, LONG, &cancel, |_| async {
            tokio::time::sleep(Duration::from_secs(10)).await;
            Ok(())
        })
        .await;
        assert!(matches!(reports[0].outcome, Err(AgentError::Cancelled { .. })));
    }
}
