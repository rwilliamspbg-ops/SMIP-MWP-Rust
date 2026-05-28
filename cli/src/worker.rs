use core_affinity::{set_for_current, CoreId};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkerAssignment {
    pub worker_index: usize,
    pub core_id: usize,
}

pub fn parse_core_list(spec: &str) -> Result<Vec<usize>, String> {
    let mut cores = Vec::new();

    for chunk in spec
        .split(',')
        .map(str::trim)
        .filter(|chunk| !chunk.is_empty())
    {
        if let Some((start, end)) = chunk.split_once('-') {
            let start = start
                .trim()
                .parse::<usize>()
                .map_err(|err| format!("invalid core id '{start}': {err}"))?;
            let end = end
                .trim()
                .parse::<usize>()
                .map_err(|err| format!("invalid core id '{end}': {err}"))?;
            if start > end {
                return Err(format!("invalid core range '{chunk}'"));
            }
            cores.extend(start..=end);
        } else {
            let core = chunk
                .parse::<usize>()
                .map_err(|err| format!("invalid core id '{chunk}': {err}"))?;
            cores.push(core);
        }
    }

    if cores.is_empty() {
        return Err("no worker cores specified".to_string());
    }

    Ok(cores)
}

pub fn build_worker_plan(worker_count: usize, core_ids: &[usize]) -> Vec<WorkerAssignment> {
    if worker_count == 0 {
        return Vec::new();
    }

    let fallback_cores: Vec<usize> = if core_ids.is_empty() {
        (0..worker_count).collect()
    } else {
        core_ids.to_vec()
    };

    (0..worker_count)
        .map(|worker_index| WorkerAssignment {
            worker_index,
            core_id: fallback_cores[worker_index % fallback_cores.len()],
        })
        .collect()
}

pub fn spawn_pinned_workers<F, T>(
    assignments: &[WorkerAssignment],
    worker_fn: F,
) -> Vec<JoinHandle<T>>
where
    F: Fn(WorkerAssignment) -> T + Send + Sync + 'static,
    T: Send + 'static,
{
    let worker_fn = Arc::new(worker_fn);

    assignments
        .iter()
        .copied()
        .map(|assignment| {
            let worker_fn = Arc::clone(&worker_fn);
            thread::spawn(move || {
                let _ = pin_current_thread(assignment.core_id);
                worker_fn(assignment)
            })
        })
        .collect()
}

pub fn pin_current_thread(core_id: usize) -> bool {
    set_for_current(CoreId { id: core_id })
}

pub fn available_core_ids() -> Vec<usize> {
    core_affinity::get_core_ids()
        .map(|cores| cores.into_iter().map(|core| core.id).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{build_worker_plan, parse_core_list};

    #[test]
    fn parses_core_lists_and_ranges() {
        let cores = parse_core_list("0-2,4,6-7").expect("core list");
        assert_eq!(cores, vec![0, 1, 2, 4, 6, 7]);
    }

    #[test]
    fn builds_round_robin_worker_plans() {
        let plan = build_worker_plan(5, &[2, 4]);
        assert_eq!(
            plan.iter()
                .map(|assignment| assignment.core_id)
                .collect::<Vec<_>>(),
            vec![2, 4, 2, 4, 2]
        );
    }
}
