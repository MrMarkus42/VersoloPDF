//! Text comparison of two documents (line based).

/// One line of a comparison result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// Line present in both documents: (page in old, page in new, text).
    Same(usize, usize, String),
    /// Line only in the old document.
    Removed(usize, String),
    /// Line only in the new document.
    Added(usize, String),
}

/// Upper bound for the dynamic-programming table (cells).
const MAX_CELLS: usize = 16_000_000;

/// Compares two documents given as (page, line) lists.
pub fn diff(old: &[(usize, String)], new: &[(usize, String)]) -> Vec<Change> {
    // Common prefix and suffix need no table.
    let prefix = old.iter().zip(new).take_while(|(a, b)| a.1 == b.1).count();
    let suffix =
        old[prefix..].iter().rev().zip(new[prefix..].iter().rev()).take_while(|(a, b)| a.1 == b.1).count();
    let a = &old[prefix..old.len() - suffix];
    let b = &new[prefix..new.len() - suffix];

    let mut out: Vec<Change> =
        old[..prefix].iter().zip(&new[..prefix]).map(|(x, y)| Change::Same(x.0, y.0, x.1.clone())).collect();
    if a.len().saturating_mul(b.len()) > MAX_CELLS {
        // Too large for an exact comparison: report the middle as replaced.
        out.extend(a.iter().map(|(p, t)| Change::Removed(*p, t.clone())));
        out.extend(b.iter().map(|(p, t)| Change::Added(*p, t.clone())));
    } else {
        // lcs[i][j] = length of the longest common subsequence of a[i..], b[j..].
        let (n, m) = (a.len(), b.len());
        let mut lcs = vec![0u32; (n + 1) * (m + 1)];
        let idx = |i: usize, j: usize| i * (m + 1) + j;
        for i in (0..n).rev() {
            for j in (0..m).rev() {
                lcs[idx(i, j)] = if a[i].1 == b[j].1 {
                    lcs[idx(i + 1, j + 1)] + 1
                } else {
                    lcs[idx(i + 1, j)].max(lcs[idx(i, j + 1)])
                };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < n || j < m {
            if i < n && j < m && a[i].1 == b[j].1 {
                out.push(Change::Same(a[i].0, b[j].0, a[i].1.clone()));
                i += 1;
                j += 1;
            } else if j < m && (i == n || lcs[idx(i, j + 1)] >= lcs[idx(i + 1, j)]) {
                out.push(Change::Added(b[j].0, b[j].1.clone()));
                j += 1;
            } else {
                out.push(Change::Removed(a[i].0, a[i].1.clone()));
                i += 1;
            }
        }
    }
    let start_old = old.len() - suffix;
    let start_new = new.len() - suffix;
    out.extend(
        old[start_old..].iter().zip(&new[start_new..]).map(|(x, y)| Change::Same(x.0, y.0, x.1.clone())),
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(v: &[&str]) -> Vec<(usize, String)> {
        v.iter().map(|s| (0, s.to_string())).collect()
    }

    #[test]
    fn finds_insertions_and_deletions() {
        let old = lines(&["a", "b", "c", "d"]);
        let new = lines(&["a", "c", "x", "d"]);
        let d = diff(&old, &new);
        assert_eq!(
            d,
            vec![
                Change::Same(0, 0, "a".into()),
                Change::Removed(0, "b".into()),
                Change::Same(0, 0, "c".into()),
                Change::Added(0, "x".into()),
                Change::Same(0, 0, "d".into()),
            ]
        );
    }

    #[test]
    fn identical_and_empty() {
        let a = lines(&["x", "y"]);
        assert!(diff(&a, &a).iter().all(|c| matches!(c, Change::Same(..))));
        assert_eq!(diff(&[], &a).len(), 2);
        assert_eq!(diff(&a, &[]).len(), 2);
    }
}
