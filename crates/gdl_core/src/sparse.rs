//! A `Vec<Option<T>>` that stores only the slots written: the collision
//! world's run-time node matrices exist for a few objects of thousands, and
//! state clones (game-run's state search) copy every slot of a vector.

use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub struct Sparse<T> {
    map: BTreeMap<usize, Option<T>>,
    len: usize,
    none: Option<T>,
}

impl<T> Sparse<T> {
    /// `len` slots, all None.
    pub fn new(len: usize) -> Self {
        Self { map: BTreeMap::new(), len, none: None }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The slots holding a value, in index order.
    pub fn values(&self) -> impl Iterator<Item = (usize, &T)> {
        self.map.iter().filter_map(|(k, v)| v.as_ref().map(|v| (*k, v)))
    }
}

impl<T> std::ops::Index<usize> for Sparse<T> {
    type Output = Option<T>;
    fn index(&self, i: usize) -> &Option<T> {
        assert!(i < self.len, "sparse slot {i} of {}", self.len);
        self.map.get(&i).unwrap_or(&self.none)
    }
}

impl<T> std::ops::IndexMut<usize> for Sparse<T> {
    fn index_mut(&mut self, i: usize) -> &mut Option<T> {
        assert!(i < self.len, "sparse slot {i} of {}", self.len);
        self.map.entry(i).or_insert(None)
    }
}

#[cfg(test)]
mod tests {
    use super::Sparse;

    #[test]
    fn reads_none_until_written_and_keeps_writes() {
        let mut s: Sparse<u32> = Sparse::new(4);
        assert_eq!(s[2], None);
        s[2] = Some(7);
        *s[3].get_or_insert(1) += 1;
        assert_eq!((s[1], s[2], s[3]), (None, Some(7), Some(2)));
        assert_eq!(s.values().collect::<Vec<_>>(), [(2, &7), (3, &2)]);
    }
}
