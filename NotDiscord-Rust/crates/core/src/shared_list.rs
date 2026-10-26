//! A copy-on-write collection for immutable snapshot payloads. The wire format
//! remains an ordinary JSON array; mutation never alters an older snapshot.
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{
    fmt,
    iter::FromIterator,
    ops::{Deref, DerefMut},
    sync::Arc,
};

pub struct SharedList<T>(Arc<Vec<T>>);

impl<T> Clone for SharedList<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
impl<T> Default for SharedList<T> {
    fn default() -> Self {
        Self::from(Vec::new())
    }
}
impl<T> SharedList<T> {
    /// Identity of immutable payloads, useful for derived presentation caches.
    pub fn same_storage(&self, other: &Self) -> bool { Arc::ptr_eq(&self.0, &other.0) }
    /// Replacing an obsolete view must not copy its shared payload merely to
    /// clear it. Dropping this handle also releases unique spare capacity.
    pub fn clear(&mut self) {
        *self = Self::default();
    }
}
impl<T: Clone> SharedList<T> {
    pub fn into_vec(self) -> Vec<T> {
        Arc::try_unwrap(self.0).unwrap_or_else(|shared| shared.as_ref().clone())
    }
}
impl<T> From<Vec<T>> for SharedList<T> {
    fn from(value: Vec<T>) -> Self {
        Self(Arc::new(value))
    }
}
impl<T: Clone> From<SharedList<T>> for Vec<T> {
    fn from(value: SharedList<T>) -> Self {
        value.into_vec()
    }
}
impl<T> FromIterator<T> for SharedList<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        Self::from(iter.into_iter().collect::<Vec<_>>())
    }
}
impl<T> Deref for SharedList<T> {
    type Target = Vec<T>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl<T: Clone> DerefMut for SharedList<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        Arc::make_mut(&mut self.0)
    }
}
impl<T> AsRef<[T]> for SharedList<T> {
    fn as_ref(&self) -> &[T] {
        self.0.as_slice()
    }
}
impl<T: Clone> Extend<T> for SharedList<T> {
    fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
        Arc::make_mut(&mut self.0).extend(iter);
    }
}
impl<T: Clone> IntoIterator for SharedList<T> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;
    fn into_iter(self) -> Self::IntoIter {
        self.into_vec().into_iter()
    }
}
impl<'a, T> IntoIterator for &'a SharedList<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}
impl<'a, T: Clone> IntoIterator for &'a mut SharedList<T> {
    type Item = &'a mut T;
    type IntoIter = std::slice::IterMut<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        Arc::make_mut(&mut self.0).iter_mut()
    }
}
impl<T: fmt::Debug> fmt::Debug for SharedList<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}
impl<T: PartialEq> PartialEq for SharedList<T> {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0) || self.0.as_ref() == other.0.as_ref()
    }
}
impl<T: Eq> Eq for SharedList<T> {}
impl<T: PartialEq> PartialEq<Vec<T>> for SharedList<T> {
    fn eq(&self, other: &Vec<T>) -> bool {
        self.0.as_ref() == other
    }
}
impl<T: PartialEq> PartialEq<SharedList<T>> for Vec<T> {
    fn eq(&self, other: &SharedList<T>) -> bool {
        self == other.0.as_ref()
    }
}
impl<T: Serialize> Serialize for SharedList<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.as_ref().serialize(serializer)
    }
}
impl<'de, T: Deserialize<'de>> Deserialize<'de> for SharedList<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Vec::<T>::deserialize(deserializer).map(Self::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Debug)]
    struct Counted(Arc<AtomicUsize>, usize);
    impl Clone for Counted {
        fn clone(&self) -> Self {
            self.0.fetch_add(1, Ordering::Relaxed);
            Self(self.0.clone(), self.1)
        }
    }
    #[test]
    fn cloning_a_snapshot_does_not_clone_elements_and_mutation_keeps_the_old_snapshot() {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut current: SharedList<_> =
            vec![Counted(calls.clone(), 1), Counted(calls.clone(), 2)].into();
        let old = current.clone();
        assert_eq!(calls.load(Ordering::Relaxed), 0);
        current[0].1 = 3;
        assert_eq!(calls.load(Ordering::Relaxed), 2);
        assert_eq!(old[0].1, 1);
        assert_eq!(current[0].1, 3);
        assert_eq!(old.len(), 2);
    }
    #[test]
    fn clearing_a_shared_view_never_clones_elements_and_drops_unique_spare_capacity() {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut current: SharedList<_> = vec![Counted(calls.clone(), 1)].into();
        let old = current.clone();
        current.clear();
        assert_eq!(calls.load(Ordering::Relaxed), 0);
        assert!(current.is_empty());
        assert_eq!(current.capacity(), 0);
        assert_eq!(old.len(), 1);
        let mut unique: SharedList<String> = Vec::with_capacity(4096).into();
        unique.clear();
        assert_eq!(unique.capacity(), 0);
    }
    #[test]
    fn json_is_the_same_array_and_owned_or_borrowed_iteration_preserves_values() {
        let original = vec![
            "20".to_owned(),
            "embed-1".to_owned(),
            "".to_owned(),
            "日本語🙂".to_owned(),
        ];
        let shared: SharedList<_> = original.clone().into();
        let bytes = serde_json::to_vec(&original).unwrap();
        assert_eq!(serde_json::to_vec(&shared).unwrap(), bytes);
        assert_eq!(
            serde_json::from_slice::<SharedList<String>>(&bytes).unwrap(),
            original
        );
        assert_eq!((&shared).into_iter().cloned().collect::<Vec<_>>(), original);
        assert_eq!(shared.clone().into_iter().collect::<Vec<_>>(), original);
        assert_eq!(shared.into_vec(), original);
        assert!(serde_json::from_str::<SharedList<String>>("null").is_err());
    }
}
