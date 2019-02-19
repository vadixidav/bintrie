mod heuristic;

pub use heuristic::*;

const HIGH: u32 = 0x8000_0000;

use std::slice;

/// Contains a list of 16 children node IDs.
///
/// `16 * 32` (`512`) bits (`64` bytes) is the size of cache lines in Intel
/// processors. This struct attempts to take advantage of that.
///
/// Each child ID's highest bit indicates if it is an internal node or a
/// leaf node.
///
/// If a child is `0` then it is empty because the root node can never be pointed to.
#[derive(Copy, Clone, Debug, Default)]
struct Internal([u32; 16]);

#[derive(Clone, Debug)]
pub struct BinTrie {
    /// The root node is always at index `0` to simplify things.
    internals: Vec<Internal>,
    /// The maximum depth to stop at.
    depth: u32,
}

impl BinTrie {
    /// Makes a new trie with a maximum `depth` of `8192`.
    ///
    /// ```
    /// # use bintrie::BinTrie;
    /// let trie = BinTrie::new();
    /// ```
    pub fn new() -> Self {
        Default::default()
    }

    /// Makes a new trie with a given maximum `depth`.
    ///
    /// ```
    /// # use bintrie::BinTrie;
    /// let trie = BinTrie::new_depth(128);
    /// ```
    pub fn new_depth(depth: u32) -> Self {
        assert!(depth > 0);
        Self {
            internals: vec![Internal::default()],
            depth,
        }
    }

    /// Inserts a number that does not have the most significant bit set.
    ///
    /// `K(n)` - A function that provides the `n`th group of `4` bits for the
    ///    key.
    /// `F(item, n)` - A function that must be able to look up the nth group
    ///    of `4` bits from a previously inserted `u32`.
    ///
    /// ```
    /// # use bintrie::BinTrie;
    /// let mut trie = BinTrie::new();
    /// // Note that the item, the key, and the lookup key all obey the
    /// // unsafe requirements.
    /// trie.insert(5, |_| 0, |_, _| 0);
    /// assert_eq!(trie.items().collect::<Vec<u32>>(), vec![5]);
    /// ```
    #[inline(always)]
    pub fn insert<K, F>(&mut self, item: u32, mut key: K, mut lookup: F)
    where
        K: FnMut(u32) -> usize,
        F: FnMut(u32, u32) -> usize,
    {
        assert!(item & HIGH == 0);
        unsafe {
            self.insert_unchecked(
                item,
                |n| {
                    let out = key(n);
                    assert!(out < 16);
                    out
                },
                |item, group| {
                    let out = lookup(item, group);
                    assert!(out < 16);
                    out
                },
            );
        }
    }

    /// Inserts a number that does not have the most significant bit set.
    ///
    /// This version is unsafe because it doesn't verify that the output
    /// of `K` and `F` are below `16`. It also doesn't verify that the
    /// `item` doesn't have its most significant bit set. Ensure these
    /// conditions are met before calling this. It still asserts
    /// that there aren't too many internal nodes.
    ///
    /// `K(n)` - A function that provides the `n`th group of `4` bits for the
    ///    key.
    /// `F(item, n)` - A function that must be able to look up the nth group
    ///    of `4` bits from a previously inserted `u32`.
    ///
    /// ```
    /// # use bintrie::BinTrie;
    /// let mut trie = BinTrie::new();
    /// // Note that the item, the key, and the lookup key all obey the
    /// // unsafe requirements.
    /// unsafe {
    ///     trie.insert_unchecked(5, |_| 0, |_, _| 0);
    /// }
    /// assert_eq!(trie.items().collect::<Vec<u32>>(), vec![5]);
    /// ```
    #[inline(always)]
    pub unsafe fn insert_unchecked<K, F>(&mut self, item: u32, mut key: K, mut lookup: F)
    where
        K: FnMut(u32) -> usize,
        F: FnMut(u32, u32) -> usize,
    {
        let mut index = 0;
        for i in 0..self.depth - 1 {
            let position = key(i);
            match *self
                .internals
                .get_unchecked(index)
                .0
                .get_unchecked(position)
            {
                // Empty node encountered.
                0 => {
                    // Insert the item in the empty spot, making sure to set
                    // its most significant bit to indicate it is a leaf.
                    *self
                        .internals
                        .get_unchecked_mut(index)
                        .0
                        .get_unchecked_mut(position) = item | HIGH;
                    // That's it.
                    return;
                }
                // Leaf node encountered.
                m if m & HIGH != 0 => {
                    // Make an empty node.
                    let mut new_internal = Internal::default();
                    // Add the existing `m` to its proper location.
                    *new_internal.0.get_unchecked_mut(lookup(m & !HIGH, i + 1)) = m;
                    // Get the index of the next internal node.
                    let new_index = self.internals.len() as u32;
                    // Panic if we go too high to fit in our indices.
                    assert!(new_index & HIGH == 0);
                    // Insert the new internal node onto the internals vector.
                    self.internals.push(new_internal);
                    // Insert the new index to the parent node.
                    *self
                        .internals
                        .get_unchecked_mut(index)
                        .0
                        .get_unchecked_mut(position) = new_index;
                    // Fallthrough to the next iteration where it will either
                    // be expanded or hit the empty leaf node position.
                    index = new_index as usize;
                }
                // Internal node encountered.
                m => {
                    // Move to the internal node.
                    index = m as usize;
                }
            }
        }

        // For the last bit we only handle the case that we can insert it.
        // The group position is `depth - 1`.
        let position = key(self.depth - 1);
        // Check if it is a leaf node.
        if *self
            .internals
            .get_unchecked(index)
            .0
            .get_unchecked(position)
            == 0
        {
            // Insert the item in the empty spot, making sure to set
            // its most significant bit to indicate it is a leaf.
            *self
                .internals
                .get_unchecked_mut(index)
                .0
                .get_unchecked_mut(position) = item | HIGH;
        }
    }

    /// Perform a lookup for a particular item.
    ///
    /// `K(n)` - A function that provides the `n`th group of `4` bits for the
    ///    key.
    ///
    /// ```
    /// # use bintrie::BinTrie;
    /// let mut trie = BinTrie::new();
    /// // Note that the item, the key, and the lookup key all obey the
    /// // unsafe requirements.
    /// let key = |_| 0;
    /// let lookup = |_, _| 0;
    /// trie.insert(5, key, lookup);
    /// assert_eq!(trie.get(key), Some(5));
    /// assert_eq!(trie.get(|_| 1), None);
    /// ```
    #[inline(always)]
    pub fn get<K>(&self, mut key: K) -> Option<u32>
    where
        K: FnMut(u32) -> usize,
    {
        unsafe {
            self.get_unchecked(|n| {
                let out = key(n);
                assert!(out < 16);
                out
            })
        }
    }

    /// Perform a lookup for a particular item.
    ///
    /// `K(n)` - A function that provides the `n`th group of `4` bits for the
    ///    key.
    ///
    /// This is unsafe to call because `key` is assumed to return indices
    /// below `16`.
    ///
    /// ```
    /// # use bintrie::BinTrie;
    /// let mut trie = BinTrie::new();
    /// // Note that the item, the key, and the lookup key all obey the
    /// // unsafe requirements.
    /// let key = |_| 0;
    /// let lookup = |_, _| 0;
    /// trie.insert(5, key, lookup);
    /// unsafe {
    ///     assert_eq!(trie.get_unchecked(key), Some(5));
    ///     assert_eq!(trie.get_unchecked(|_| 1), None);
    /// }
    /// ```
    #[inline(always)]
    pub unsafe fn get_unchecked<K>(&self, mut key: K) -> Option<u32>
    where
        K: FnMut(u32) -> usize,
    {
        let mut index = 0;
        for i in 0..self.depth {
            match *self.internals.get_unchecked(index).0.get_unchecked(key(i)) {
                // Empty node encountered.
                0 => {
                    return None;
                }
                // Leaf node encountered.
                m if m & HIGH != 0 => return Some(m & !HIGH),
                // Internal node encountered.
                m => {
                    // Move to the internal node.
                    index = m as usize;
                }
            }
        }
        None
    }

    /// Get an iterator over the items added to the trie.
    ///
    /// ```
    /// # use bintrie::BinTrie;
    /// let mut trie = BinTrie::new();
    /// trie.insert(3, |_| 0, |_, _| 0);
    /// assert_eq!(trie.items().collect::<Vec<u32>>(), vec![3]);
    /// ```
    pub fn items<'a>(&'a self) -> impl Iterator<Item = u32> + 'a {
        Iter::new(self)
    }

    /// Iterates over the trie while using the `heuristic` to guide iteration.
    ///
    /// This can be used to limit the search space or to guide the search space
    /// for a fast k-NN or other spatial heuristic search.
    ///
    /// `heuristic` must implement `UncheckedHeuristic`, which the normal
    /// `Heuristic` trait satisfies. Implement `Heuristic` unless you are sure
    /// that you need `UncheckedHeuristic`, which is **unsafe** to implement.
    ///
    /// ```
    /// # use bintrie::{BinTrie, FnHeuristic};
    /// let mut trie = BinTrie::new();
    /// let lookup = |n, l| match n {
    ///     3 => 0,
    ///     5 => if l == 1 { 1 } else {0},
    ///     _ => 0,
    /// };
    /// trie.insert(3, |n| lookup(3, n), lookup);
    /// trie.insert(5, |n| lookup(5, n), lookup);
    /// assert_eq!(trie.explore(FnHeuristic(|n| n < 2)).collect::<Vec<u32>>(), vec![3, 5]);
    /// assert_eq!(trie.explore(FnHeuristic(|n| n == 0)).collect::<Vec<u32>>(), vec![3]);
    /// let mut level = 0;
    /// assert_eq!(trie.explore(FnHeuristic(move |n| {
    ///     level += 1;
    ///     match level {
    ///         1 => n == 0,
    ///         2 => n == 1,
    ///         _ => false,
    ///     }
    /// })).collect::<Vec<u32>>(), vec![5]);
    /// ```
    pub fn explore<'a, H>(&'a self, heuristic: H) -> impl Iterator<Item = u32> + 'a
    where
        H: IntoHeuristic,
        H::Heuristic: 'a,
    {
        ExploreIter::new(self, heuristic.into_heuristic())
    }
}

impl Default for BinTrie {
    fn default() -> Self {
        Self {
            internals: vec![Internal::default()],
            depth: 8192,
        }
    }
}

struct Iter<'a> {
    trie: &'a BinTrie,
    indices: Vec<slice::Iter<'a, u32>>,
}

impl<'a> Iter<'a> {
    fn new(trie: &'a BinTrie) -> Self {
        Self {
            trie,
            indices: vec![trie.internals[0].0.iter()],
        }
    }
}

impl<'a> Iterator for Iter<'a> {
    type Item = u32;
    #[inline(always)]
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            // Get the current slice. If there is none, then we return `None`.
            let mut current = self.indices.pop()?;
            // Get the next item in the slice or continue the loop if its empty.
            let n = if let Some(n) = current.next() {
                // Push the slice back.
                self.indices.push(current);
                n
            } else {
                continue;
            };
            // Check what kind of node it is.
            match n {
                // Empty node
                0 => {}
                // Leaf node
                n if n & HIGH != 0 => {
                    return Some(n & !HIGH);
                }
                // Internal node
                &n => self.indices.push(self.trie.internals[n as usize].0.iter()),
            }
        }
    }
}

struct ExploreIter<'a, H>
where
    H: UncheckedHeuristic,
{
    trie: &'a BinTrie,
    indices: Vec<(&'a [u32; 16], H, H::UncheckedIter)>,
}

impl<'a, H> ExploreIter<'a, H>
where
    H: UncheckedHeuristic,
{
    fn new(trie: &'a BinTrie, heuristic: H) -> Self {
        let iter = heuristic.iter_unchecked();
        Self {
            trie,
            indices: vec![(&trie.internals[0].0, heuristic, iter)],
        }
    }
}

impl<'a, H> Iterator for ExploreIter<'a, H>
where
    H: UncheckedHeuristic,
{
    type Item = u32;
    #[inline(always)]
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            // Get the current array, heuristic, and iter.
            // If there is none, then we return `None`.
            let (array, heuristic, mut iter) = self.indices.pop()?;
            // Clone the heuristic before we put it back so we can
            // use it when descending further.
            let mut next_heuristic = heuristic.clone();
            // Get the next item in the array or continue the loop if its empty.
            let (choice, n) = if let Some(choice) = iter.next() {
                let n = unsafe { array.get_unchecked(choice) };
                // Push the state back.
                self.indices.push((array, heuristic, iter));
                (choice, n)
            } else {
                continue;
            };
            // Check what kind of node it is.
            match n {
                // Empty node
                0 => {}
                // Leaf node
                n if n & HIGH != 0 => {
                    return Some(n & !HIGH);
                }
                // Internal node
                &n => {
                    next_heuristic.enter_unchecked(choice);
                    let iter = next_heuristic.iter_unchecked();
                    self.indices
                        .push((&self.trie.internals[n as usize].0, next_heuristic, iter))
                }
            }
        }
    }
}
