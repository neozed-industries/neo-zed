use std::collections::HashMap;
use std::hash::Hash;

/// Computes the minimum detail level needed for each item so that no two items
/// share the same description. Items whose descriptions are unique at level 0
/// stay at 0; items that collide get their detail level incremented until either
/// the collision is resolved or increasing the level no longer changes the
/// description (preventing infinite loops for truly identical items).
pub fn compute_disambiguation_details<T, D>(
    items: &[T],
    get_description: impl Fn(&T, usize) -> D,
) -> Vec<usize>
where
    D: Eq + Hash + Clone,
{
    let mut details = vec![0usize; items.len()];
    let mut descriptions: HashMap<D, Vec<usize>> = HashMap::default();
    let mut current_descriptions: Vec<D> =
        items.iter().map(|item| get_description(item, 0)).collect();

    loop {
        let mut any_collisions = false;

        for (index, (item, &detail)) in items.iter().zip(&details).enumerate() {
            if detail > 0 {
                let new_description = get_description(item, detail);
                if new_description == current_descriptions[index] {
                    continue;
                }
                current_descriptions[index] = new_description;
            }
            descriptions
                .entry(current_descriptions[index].clone())
                .or_insert_with(Vec::new)
                .push(index);
        }

        for (_, indices) in descriptions.drain() {
            if indices.len() > 1 {
                any_collisions = true;
                for index in indices {
                    details[index] += 1;
                }
            }
        }

        if !any_collisions {
            break;
        }
    }

    details
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_conflicts() {
        let items = vec!["alpha", "beta", "gamma"];
        let details = compute_disambiguation_details(&items, |item, _detail| item.to_string());
        assert_eq!(details, vec![0, 0, 0]);
    }

    #[test]
    fn test_simple_two_way_conflict() {
        // Two items with the same base name but different parents.
        let items = vec![("src/foo.rs", "foo.rs"), ("lib/foo.rs", "foo.rs")];
        let details = compute_disambiguation_details(&items, |item, detail| match detail {
            0 => item.1.to_string(),
            _ => item.0.to_string(),
        });
        assert_eq!(details, vec![1, 1]);
    }

    #[test]
    fn test_three_way_conflict() {
        let items = vec![
            ("foo.rs", "a/foo.rs"),
            ("foo.rs", "b/foo.rs"),
            ("foo.rs", "c/foo.rs"),
        ];
        let details = compute_disambiguation_details(&items, |item, detail| match detail {
            0 => item.0.to_string(),
            _ => item.1.to_string(),
        });
        assert_eq!(details, vec![1, 1, 1]);
    }

    #[test]
    fn test_deeper_conflict() {
        // At detail 0, all three show "file.rs".
        // At detail 1, items 0 and 1 both show "src/file.rs", item 2 shows "lib/file.rs".
        // At detail 2, item 0 shows "a/src/file.rs", item 1 shows "b/src/file.rs".
        let items = vec![
            vec!["file.rs", "src/file.rs", "a/src/file.rs"],
            vec!["file.rs", "src/file.rs", "b/src/file.rs"],
            vec!["file.rs", "lib/file.rs", "x/lib/file.rs"],
        ];
        let details = compute_disambiguation_details(&items, |item, detail| {
            let clamped = detail.min(item.len() - 1);
            item[clamped].to_string()
        });
        assert_eq!(details, vec![2, 2, 1]);
    }

    #[test]
    fn test_mixed_conflicting_and_unique() {
        let items = vec![
            ("src/foo.rs", "foo.rs"),
            ("lib/foo.rs", "foo.rs"),
            ("src/bar.rs", "bar.rs"),
        ];
        let details = compute_disambiguation_details(&items, |item, detail| match detail {
            0 => item.1.to_string(),
            _ => item.0.to_string(),
        });
        assert_eq!(details, vec![1, 1, 0]);
    }

    #[test]
    fn test_identical_items_terminates() {
        // All items return the same description at every detail level.
        // The algorithm must terminate rather than looping forever.
        let items = vec!["same", "same", "same"];
        let details = compute_disambiguation_details(&items, |item, _detail| item.to_string());
        // After bumping to 1, the description doesn't change from level 0,
        // so the items are skipped and the loop terminates.
        assert_eq!(details, vec![1, 1, 1]);
    }

    #[test]
    fn test_single_item() {
        let items = vec!["only"];
        let details = compute_disambiguation_details(&items, |item, _detail| item.to_string());
        assert_eq!(details, vec![0]);
    }

    #[test]
    fn test_empty_input() {
        let items: Vec<&str> = vec![];
        let details = compute_disambiguation_details(&items, |item, _detail| item.to_string());
        let expected: Vec<usize> = vec![];
        assert_eq!(details, expected);
    }
}
