use std::collections::BTreeSet;
use std::path::PathBuf;

use crate::graph::FileCycle;

pub fn collect_cyclic_files(cycles: &[FileCycle]) -> Vec<PathBuf> {
    cycles
        .iter()
        .flat_map(|cycle| cycle.iter().cloned())
        .collect::<BTreeSet<PathBuf>>()
        .into_iter()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn empty_input_returns_empty() {
        assert!(collect_cyclic_files(&[]).is_empty());
    }

    #[test]
    fn single_cycle_sorted() {
        let cycles = vec![vec![PathBuf::from("b.py"), PathBuf::from("a.py")]];

        let result = collect_cyclic_files(&cycles);

        assert_eq!(result, vec![PathBuf::from("a.py"), PathBuf::from("b.py")]);
    }

    #[test]
    fn overlapping_cycles_deduped() {
        let cycles = vec![
            vec![PathBuf::from("a.py"), PathBuf::from("b.py")],
            vec![PathBuf::from("b.py"), PathBuf::from("c.py")],
        ];

        let result = collect_cyclic_files(&cycles);

        assert_eq!(
            result,
            vec![
                PathBuf::from("a.py"),
                PathBuf::from("b.py"),
                PathBuf::from("c.py"),
            ]
        );
    }

    #[test]
    fn deterministic_regardless_of_order() {
        let first = vec![
            vec![PathBuf::from("c.py"), PathBuf::from("a.py")],
            vec![PathBuf::from("b.py"), PathBuf::from("a.py")],
        ];
        let second = vec![
            vec![PathBuf::from("b.py"), PathBuf::from("a.py")],
            vec![PathBuf::from("a.py"), PathBuf::from("c.py")],
        ];

        assert_eq!(collect_cyclic_files(&first), collect_cyclic_files(&second));
    }
}
