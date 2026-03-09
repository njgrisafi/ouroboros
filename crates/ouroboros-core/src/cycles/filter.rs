use crate::config::CyclesConfig;
use crate::graph::FileCycle;

/// Filter cycles (SCCs) by size using the given configuration.
///
/// Retains only cycles whose length is within `[min_scc_size, max_scc_size]`.
/// If `max_scc_size` is `None`, there is no upper bound.
pub fn filter_cycles_by_size(
    cycles: Vec<FileCycle>,
    config: &CyclesConfig,
) -> Vec<FileCycle> {
    cycles
        .into_iter()
        .filter(|cycle| {
            let size = cycle.len();
            if size < config.min_scc_size {
                return false;
            }
            if let Some(max) = config.max_scc_size {
                if size > max {
                    return false;
                }
            }
            true
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Helper: create a cycle of `n` dummy files.
    fn make_cycle(n: usize) -> FileCycle {
        (0..n)
            .map(|i| PathBuf::from(format!("file_{i}.py")))
            .collect()
    }

    #[test]
    fn filter_to_exact_size_2() {
        let cycles = vec![make_cycle(2), make_cycle(3), make_cycle(5)];
        let config = CyclesConfig {
            min_scc_size: 2,
            max_scc_size: Some(2),
        };
        let result = filter_cycles_by_size(cycles, &config);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].len(), 2);
    }

    #[test]
    fn filter_range_2_to_5() {
        let cycles = vec![
            make_cycle(1),
            make_cycle(2),
            make_cycle(4),
            make_cycle(6),
        ];
        let config = CyclesConfig {
            min_scc_size: 2,
            max_scc_size: Some(5),
        };
        let result = filter_cycles_by_size(cycles, &config);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].len(), 2);
        assert_eq!(result[1].len(), 4);
    }

    #[test]
    fn filter_no_max() {
        let cycles = vec![make_cycle(1), make_cycle(2), make_cycle(10)];
        let config = CyclesConfig {
            min_scc_size: 2,
            max_scc_size: None,
        };
        let result = filter_cycles_by_size(cycles, &config);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].len(), 2);
        assert_eq!(result[1].len(), 10);
    }

    #[test]
    fn filter_empty_input() {
        let cycles: Vec<FileCycle> = vec![];
        let config = CyclesConfig::default();
        let result = filter_cycles_by_size(cycles, &config);
        assert!(result.is_empty());
    }

    #[test]
    fn filter_all_removed() {
        let cycles = vec![make_cycle(1)];
        let config = CyclesConfig {
            min_scc_size: 2,
            max_scc_size: None,
        };
        let result = filter_cycles_by_size(cycles, &config);
        assert!(result.is_empty());
    }
}
