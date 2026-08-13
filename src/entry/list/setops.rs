#[derive(Clone, Copy)]
pub(super) struct UniverseView<'a>(pub(super) &'a [(i64, i64)]);

impl<'a> UniverseView<'a> {
    pub(super) fn len(self) -> usize {
        self.0.len()
    }

    pub(super) fn intersect_sorted(self, right: &[i64]) -> Vec<i64> {
        intersect_pairs_sorted(self.0, right)
    }

    pub(super) fn difference_sorted(self, right: &[i64]) -> Vec<i64> {
        difference_pairs_sorted(self.0, right)
    }
}

fn intersect_pairs_sorted(left: &[(i64, i64)], right: &[i64]) -> Vec<i64> {
    let mut result = Vec::with_capacity(left.len().min(right.len()));
    let mut left_index = 0usize;
    let mut right_index = 0usize;
    while left_index < left.len() && right_index < right.len() {
        match left[left_index].0.cmp(&right[right_index]) {
            std::cmp::Ordering::Less => left_index += 1,
            std::cmp::Ordering::Greater => right_index += 1,
            std::cmp::Ordering::Equal => {
                result.push(left[left_index].0);
                left_index += 1;
                right_index += 1;
            }
        }
    }
    result
}

pub(super) fn intersect_sorted_into(left: &[i64], right: &[i64], result: &mut Vec<i64>) {
    result.clear();
    let target_capacity = left.len().min(right.len());
    result.reserve(target_capacity.saturating_sub(result.capacity()));
    let mut left_index = 0usize;
    let mut right_index = 0usize;
    while left_index < left.len() && right_index < right.len() {
        match left[left_index].cmp(&right[right_index]) {
            std::cmp::Ordering::Less => left_index += 1,
            std::cmp::Ordering::Greater => right_index += 1,
            std::cmp::Ordering::Equal => {
                result.push(left[left_index]);
                left_index += 1;
                right_index += 1;
            }
        }
    }
}

fn difference_pairs_sorted(left: &[(i64, i64)], right: &[i64]) -> Vec<i64> {
    let mut result = Vec::with_capacity(left.len());
    let mut left_index = 0usize;
    let mut right_index = 0usize;
    while left_index < left.len() {
        if right_index >= right.len() {
            result.extend(left[left_index..].iter().map(|(entry_pk, _)| *entry_pk));
            break;
        }
        match left[left_index].0.cmp(&right[right_index]) {
            std::cmp::Ordering::Less => {
                result.push(left[left_index].0);
                left_index += 1;
            }
            std::cmp::Ordering::Greater => right_index += 1,
            std::cmp::Ordering::Equal => {
                left_index += 1;
                right_index += 1;
            }
        }
    }
    result
}

pub(super) fn merge_union_sorted_into(left: &[i64], right: &[i64], result: &mut Vec<i64>) {
    result.clear();
    let target_capacity = left.len() + right.len();
    result.reserve(target_capacity.saturating_sub(result.capacity()));
    let mut left_index = 0usize;
    let mut right_index = 0usize;
    while left_index < left.len() && right_index < right.len() {
        match left[left_index].cmp(&right[right_index]) {
            std::cmp::Ordering::Less => {
                result.push(left[left_index]);
                left_index += 1;
            }
            std::cmp::Ordering::Greater => {
                result.push(right[right_index]);
                right_index += 1;
            }
            std::cmp::Ordering::Equal => {
                result.push(left[left_index]);
                left_index += 1;
                right_index += 1;
            }
        }
    }
    result.extend_from_slice(&left[left_index..]);
    result.extend_from_slice(&right[right_index..]);
}

#[cfg(test)]
fn intersect_sorted(left: &[i64], right: &[i64]) -> Vec<i64> {
    let mut result = Vec::with_capacity(left.len().min(right.len()));
    intersect_sorted_into(left, right, &mut result);
    result
}

#[cfg(test)]
fn merge_union_sorted(left: &[i64], right: &[i64]) -> Vec<i64> {
    let mut result = Vec::with_capacity(left.len() + right.len());
    merge_union_sorted_into(left, right, &mut result);
    result
}

#[cfg(test)]
mod tests {
    use super::{UniverseView, intersect_sorted, merge_union_sorted};

    #[test]
    fn intersect_sorted_returns_common_values_in_order() {
        assert_eq!(intersect_sorted(&[1, 3, 4, 7], &[2, 3, 4, 8]), vec![3, 4]);
    }

    #[test]
    fn difference_sorted_returns_values_missing_from_right() {
        let left = [(1, 0), (2, 0), (3, 0), (5, 0)];
        assert_eq!(
            UniverseView(&left).difference_sorted(&[2, 4, 5]),
            vec![1, 3]
        );
    }

    #[test]
    fn merge_union_sorted_merges_without_duplicates() {
        assert_eq!(
            merge_union_sorted(&[1, 2, 5, 9], &[2, 3, 4, 9]),
            vec![1, 2, 3, 4, 5, 9]
        );
    }

    #[test]
    fn merge_union_sorted_handles_empty_inputs() {
        assert_eq!(merge_union_sorted(&[], &[2, 4]), vec![2, 4]);
        assert_eq!(merge_union_sorted(&[1, 3], &[]), vec![1, 3]);
    }
}
