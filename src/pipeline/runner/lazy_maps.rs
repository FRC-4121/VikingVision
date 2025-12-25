//! Lazy collections for the inputs and outputs of compoennts.
//!
//! All of these types are thin wrappers around references and are therefore cheap to
//! construct and copy.

use super::{InputIndex, RunnerComponentId};
use smol_str::SmolStr;
use std::borrow::Borrow;
use std::collections::hash_map::*;
use std::fmt::{self, Debug, Formatter};
use std::hash::Hash;

#[derive(Clone, Copy)]
pub(super) enum InputSetInner<'r> {
    Primary,
    SingleNamed(&'r SmolStr),
    Map(&'r HashMap<SmolStr, InputIndex>),
}
#[derive(Clone)]
enum InputSetIterInner<'r> {
    Empty,
    One(&'r SmolStr),
    Many(Keys<'r, SmolStr, InputIndex>),
}

/// The iterator returned from all of the iterator methods for [`InputSet`]
#[derive(Clone)]
pub struct InputSetIter<'r>(InputSetIterInner<'r>);
impl<'r> Iterator for InputSetIter<'r> {
    type Item = &'r SmolStr;

    fn next(&mut self) -> Option<Self::Item> {
        match &mut self.0 {
            InputSetIterInner::Empty => None,
            InputSetIterInner::One(v) => {
                let out = *v;
                self.0 = InputSetIterInner::Empty;
                Some(out)
            }
            InputSetIterInner::Many(it) => it.next(),
        }
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.len();
        (len, Some(len))
    }
}
impl<'r> ExactSizeIterator for InputSetIter<'r> {
    fn len(&self) -> usize {
        match &self.0 {
            InputSetIterInner::Empty => 0,
            InputSetIterInner::One(_) => 1,
            InputSetIterInner::Many(it) => it.len(),
        }
    }
}
impl<'r> std::iter::FusedIterator for InputSetIter<'r> {}

/// A lazy set of inputs for a component.
///
/// This doesn't allocate, it borrows from the runner.
#[derive(Clone, Copy)]
pub struct InputSet<'r>(pub(super) InputSetInner<'r>);
impl<'r> InputSet<'r> {
    pub fn contains<Q: Hash + Eq + ?Sized>(&self, chan: &Q) -> bool
    where
        SmolStr: Borrow<Q>,
    {
        match &self.0 {
            InputSetInner::Primary => false,
            InputSetInner::SingleNamed(v) => Borrow::<Q>::borrow(*v) == chan,
            InputSetInner::Map(m) => m.contains_key(chan),
        }
    }
    pub fn iter(&self) -> InputSetIter<'r> {
        InputSetIter(match &self.0 {
            InputSetInner::Primary => InputSetIterInner::Empty,
            InputSetInner::SingleNamed(v) => InputSetIterInner::One(v),
            InputSetInner::Map(m) => InputSetIterInner::Many(m.keys()),
        })
    }
    pub fn len(&self) -> usize {
        match &self.0 {
            InputSetInner::Primary => 0,
            InputSetInner::SingleNamed(..) => 1,
            InputSetInner::Map(m) => m.len(),
        }
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// An input set can be empty either because no named inputs were passed or because it takes a single primary input.
    ///
    /// This can be used to check which reason it is.
    pub fn is_primary(&self) -> bool {
        matches!(self.0, InputSetInner::Primary)
    }
}
impl<'r> IntoIterator for InputSet<'r> {
    type IntoIter = InputSetIter<'r>;
    type Item = &'r SmolStr;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}
impl<'r> IntoIterator for &InputSet<'r> {
    type IntoIter = InputSetIter<'r>;
    type Item = &'r SmolStr;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}
impl Debug for InputSet<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_set().entries(self.iter()).finish()
    }
}

/// The iterator returned from [`ListenerMap::iter`] and the [`IntoIterator`] implementations
#[derive(Clone)]
pub struct ListenerMapIter<'r>(
    Iter<'r, SmolStr, Vec<(RunnerComponentId, InputIndex, Option<u32>)>>,
);
impl<'r> Iterator for ListenerMapIter<'r> {
    type Item = (&'r SmolStr, usize);

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(|(k, v)| (k, v.len()))
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.0.size_hint()
    }
}
impl<'r> ExactSizeIterator for ListenerMapIter<'r> {
    fn len(&self) -> usize {
        self.0.len()
    }
}
impl<'r> std::iter::FusedIterator for ListenerMapIter<'r> {}

/// The iterator returned from [`ListenerMap::keys`]
#[derive(Clone)]
pub struct ListenerMapKeys<'r>(
    Keys<'r, SmolStr, Vec<(RunnerComponentId, InputIndex, Option<u32>)>>,
);
impl<'r> Iterator for ListenerMapKeys<'r> {
    type Item = &'r SmolStr;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next()
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.0.size_hint()
    }
}
impl<'r> ExactSizeIterator for ListenerMapKeys<'r> {
    fn len(&self) -> usize {
        self.0.len()
    }
}
impl<'r> std::iter::FusedIterator for ListenerMapKeys<'r> {}

/// A map of how many listeners are on each output channel of a component
///
/// This borrows from the pipeline runner to avoid allocation. The value type
/// is the number of listeners, but since it's lazy, this can't implement [`Index`](std::ops::Index).
#[derive(Clone, Copy)]
pub struct ListenerMap<'r>(
    pub(super) &'r HashMap<SmolStr, Vec<(RunnerComponentId, InputIndex, Option<u32>)>>,
);
impl<'r> ListenerMap<'r> {
    pub fn iter(&self) -> ListenerMapIter<'r> {
        ListenerMapIter(self.0.iter())
    }
    pub fn keys(&self) -> ListenerMapKeys<'r> {
        ListenerMapKeys(self.0.keys())
    }
    pub fn contains_key<Q: Hash + Eq + ?Sized>(&self, chan: &Q) -> bool
    where
        SmolStr: Borrow<Q>,
    {
        self.0.contains_key(chan)
    }
    pub fn get<Q: Hash + Eq + ?Sized>(&self, chan: &Q) -> Option<usize>
    where
        SmolStr: Borrow<Q>,
    {
        self.0.get(chan).map(Vec::len)
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
impl<'r> IntoIterator for ListenerMap<'r> {
    type IntoIter = ListenerMapIter<'r>;
    type Item = (&'r SmolStr, usize);

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}
impl<'r> IntoIterator for &ListenerMap<'r> {
    type IntoIter = ListenerMapIter<'r>;
    type Item = (&'r SmolStr, usize);

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}
impl Debug for ListenerMap<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.iter()).finish()
    }
}

#[derive(Clone)]
enum InputIndexMapIterInner<'r> {
    Single(Option<&'r SmolStr>),
    Many(Iter<'r, SmolStr, InputIndex>, u32),
}

/// The iterator returned from [`InputIndexMap::iter`]
#[derive(Clone)]
pub struct InputIndexMapIter<'r>(InputIndexMapIterInner<'r>);
impl<'r> Iterator for InputIndexMapIter<'r> {
    type Item = (&'r SmolStr, InputIndex);
    fn next(&mut self) -> Option<Self::Item> {
        match &mut self.0 {
            InputIndexMapIterInner::Single(o) => o.take().map(|n| (n, InputIndex(0, 0))),
            InputIndexMapIterInner::Many(it, prune) => it
                .next()
                .map(|(k, InputIndex(r, c))| (k, InputIndex(r - *prune, *c))),
        }
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.len();
        (len, Some(len))
    }
}
impl<'r> ExactSizeIterator for InputIndexMapIter<'r> {
    fn len(&self) -> usize {
        match &self.0 {
            InputIndexMapIterInner::Single(None) => 0,
            InputIndexMapIterInner::Single(Some(_)) => 1,
            InputIndexMapIterInner::Many(it, _) => it.len(),
        }
    }
}
impl<'r> std::iter::FusedIterator for InputIndexMapIter<'r> {}

enum InputIndexMapKeysInner<'r> {
    Single(Option<&'r SmolStr>),
    Many(Keys<'r, SmolStr, InputIndex>),
}

/// The iterator returned from [`InputIndexMap::keys`]
pub struct InputIndexMapKeys<'r>(InputIndexMapKeysInner<'r>);
impl<'r> Iterator for InputIndexMapKeys<'r> {
    type Item = &'r SmolStr;
    fn next(&mut self) -> Option<Self::Item> {
        match &mut self.0 {
            InputIndexMapKeysInner::Single(o) => o.take(),
            InputIndexMapKeysInner::Many(it) => it.next(),
        }
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.len();
        (len, Some(len))
    }
}
impl<'r> ExactSizeIterator for InputIndexMapKeys<'r> {
    fn len(&self) -> usize {
        match &self.0 {
            InputIndexMapKeysInner::Single(None) => 0,
            InputIndexMapKeysInner::Single(Some(_)) => 1,
            InputIndexMapKeysInner::Many(it) => it.len(),
        }
    }
}
impl<'r> std::iter::FusedIterator for InputIndexMapKeys<'r> {}

#[derive(Clone, Copy)]
pub(super) enum InputIndexMapInner<'r> {
    Single(&'r SmolStr),
    Many(&'r HashMap<SmolStr, InputIndex>, u32),
}

/// A map of input channels to [`InputIndex`]es that can be used on an input tree.
///
/// This doesn't allocate, and borrows from the runner.
#[derive(Clone, Copy)]
pub struct InputIndexMap<'r>(pub(super) InputIndexMapInner<'r>);
impl<'r> InputIndexMap<'r> {
    pub fn iter(&self) -> InputIndexMapIter<'r> {
        match &self.0 {
            InputIndexMapInner::Single(s) => {
                InputIndexMapIter(InputIndexMapIterInner::Single(Some(s)))
            }
            InputIndexMapInner::Many(m, p) => {
                InputIndexMapIter(InputIndexMapIterInner::Many(m.iter(), *p))
            }
        }
    }
    pub fn keys(&self) -> InputIndexMapKeys<'r> {
        match &self.0 {
            InputIndexMapInner::Single(s) => {
                InputIndexMapKeys(InputIndexMapKeysInner::Single(Some(s)))
            }
            InputIndexMapInner::Many(m, _) => {
                InputIndexMapKeys(InputIndexMapKeysInner::Many(m.keys()))
            }
        }
    }
    pub fn contains_key<Q: Hash + Eq + ?Sized>(&self, chan: &Q) -> bool
    where
        SmolStr: Borrow<Q>,
    {
        match &self.0 {
            InputIndexMapInner::Single(name) => Borrow::<Q>::borrow(*name) == chan,
            InputIndexMapInner::Many(m, _) => m.contains_key(chan),
        }
    }
    pub fn get<Q: Hash + Eq + ?Sized>(&self, chan: &Q) -> Option<InputIndex>
    where
        SmolStr: Borrow<Q>,
    {
        match &self.0 {
            InputIndexMapInner::Single(name) => {
                (Borrow::<Q>::borrow(*name) == chan).then_some(InputIndex(0, 0))
            }
            InputIndexMapInner::Many(m, prune) => m
                .get(chan)
                .map(|InputIndex(t, c)| InputIndex(t - *prune, *c)),
        }
    }
    pub fn len(&self) -> usize {
        match &self.0 {
            InputIndexMapInner::Single(_) => 1,
            InputIndexMapInner::Many(m, _) => m.len(),
        }
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
impl<'r> IntoIterator for InputIndexMap<'r> {
    type IntoIter = InputIndexMapIter<'r>;
    type Item = (&'r SmolStr, InputIndex);

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}
impl<'r> IntoIterator for &InputIndexMap<'r> {
    type IntoIter = InputIndexMapIter<'r>;
    type Item = (&'r SmolStr, InputIndex);

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}
impl Debug for InputIndexMap<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.iter()).finish()
    }
}
