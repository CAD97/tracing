use super::anymap::{AnyMap, TypeMap};
use crate::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};
use core::any::type_name;
use sharded_slab::Pool;
use std::{any::TypeId, fmt};

type ExtPool<T> = Pool<Option<T>>;

/// An immutable, read-only reference to a Span's extensions.
#[cfg_attr(docsrs, doc(cfg(feature = "std")))]
pub struct Extensions<'a> {
    // T => ExtPool<T>
    store: RwLockReadGuard<'a, AnyMap>,
    keys: RwLockReadGuard<'a, TypeMap<usize>>,
}

impl<'a> Extensions<'a> {
    #[cfg(feature = "registry")]
    pub(crate) fn new(
        store: RwLockReadGuard<'a, AnyMap>,
        keys: RwLockReadGuard<'a, TypeMap<usize>>,
    ) -> Self {
        Self { store, keys }
    }

    /// Immutably borrows a type previously inserted into this `Extensions`.
    pub fn get<T: 'static>(&self) -> Option<&T> {
        let &key = self.keys.get(&TypeId::of::<T>())?;
        let pool = self.store.get::<ExtPool<T>>()?;
        let ext = pool.get(key)?;
        ext.as_ref()
    }
}

/// An mutable reference to a Span's extensions.
#[cfg_attr(docsrs, doc(cfg(feature = "std")))]
pub struct ExtensionsMut<'a> {
    // T => ExtPool<T>
    store: &'a RwLock<AnyMap>,
    keys: RwLockWriteGuard<'a, TypeMap<usize>>,
}

impl<'a> ExtensionsMut<'a> {
    #[cfg(feature = "registry")]
    pub(crate) fn new(
        store: &'a RwLock<AnyMap>,
        keys: RwLockWriteGuard<'a, TypeMap<usize>>,
    ) -> Self {
        Self { store, keys }
    }

    /// Insert a type into this `Extensions`.
    ///
    /// Note that extensions are _not_
    /// [subscriber]-specific—they are _span_-specific. This means that
    /// other subscribers can access and mutate extensions that
    /// a different Subscriber recorded. For example, an application might
    /// have a subscriber that records execution timings, alongside a subscriber
    /// that reports spans and events to a distributed
    /// tracing system that requires timestamps for spans.
    /// Ideally, if one subscriber records a timestamp _x_, the other subscriber
    /// should be able to reuse timestamp _x_.
    ///
    /// Therefore, extensions should generally be newtypes, rather than common
    /// types like [`String`](std::string::String), to avoid accidental
    /// cross-`Subscriber` clobbering.
    ///
    /// ## Panics
    ///
    /// If `T` is already present in `Extensions`, then this method will panic.
    ///
    /// [subscriber]: crate::subscribe::Subscribe
    pub fn insert<T: Send + Sync + 'static>(&mut self, val: T) {
        if self.keys.contains_key(&TypeId::of::<T>()) {
            panic!(
                "Extensions already contain a value for type `{:?}`",
                type_name::<T>()
            );
        }

        // We try a read lock first to reduce contention on the global RwLock.
        let mut store = self.store.read().expect("Mutex poisoned");
        let pool = match store.get::<ExtPool<T>>() {
            Some(pool) => pool,
            None => {
                drop(store);
                self.store
                    .write()
                    .expect("Mutex poisoned")
                    .insert(Box::new(ExtPool::<T>::default()));
                store = self.store.read().expect("Mutex poisoned");
                store.get().unwrap()
            }
        };

        let key = pool
            .create_with(|place| *place = Some(val))
            .expect("Unable to allocate another span extension");

        self.keys.insert(TypeId::of::<T>(), key);
    }

    /// Replaces an existing `T` into this extensions.
    ///
    /// If `T` is not present, `Option::None` will be returned.
    pub fn replace<T: Send + Sync + 'static>(&mut self, val: T) -> Option<()> {
        let FIXME_BREAKING_CHANGE = ();

        let previous = self.remove::<T>();
        self.insert(val);
        previous
    }

    /// Get a mutable reference to a type previously inserted on this `ExtensionsMut`.
    pub fn get_mut<T: 'static>(&mut self) -> Option<&mut T> {
        let &key = self.keys.get(&TypeId::of::<T>())?;
        let store = self.store.read().expect("Mutex poisoned");
        let pool = store.get::<ExtPool<T>>()?;
        let ext = pool.get(key)?;

        ext.as_mut()
    }

    /// Remove a type from this `Extensions`.
    ///
    /// If a extension of this type existed, it will be returned.
    pub fn remove<T: Send + Sync + 'static>(&mut self) -> Option<()> {
        let FIXME_BREAKING_CHANGE = ();

        let store = self.store.read().expect("Mutex poisoned");
        self.keys.remove(&TypeId::of::<T>()).map(|key| {
            store
                .get::<ExtPool<T>>()
                .expect("Extensions corrupted")
                .clear(key); // FIXME(CAD97): s/clear(key);/remove(key)/
        })
    }
}

impl fmt::Debug for Extensions<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Extensions")
            .field("len", &self.keys.len())
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for ExtensionsMut<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Extensions")
            .field("len", &self.keys.len())
            .finish_non_exhaustive()
    }
}

// #[cfg(test)]
// mod tests {
//     use super::*;

//     #[derive(Debug, PartialEq)]
//     struct MyType(i32);

//     #[test]
//     fn test_extensions() {
//         let mut extensions = ExtensionsInner::new();

//         extensions.insert(5i32);
//         extensions.insert(MyType(10));

//         assert_eq!(extensions.get(), Some(&5i32));
//         assert_eq!(extensions.get_mut(), Some(&mut 5i32));

//         assert_eq!(extensions.remove::<i32>(), Some(5i32));
//         assert!(extensions.get::<i32>().is_none());

//         assert_eq!(extensions.get::<bool>(), None);
//         assert_eq!(extensions.get(), Some(&MyType(10)));
//     }

//     #[test]
//     fn clear_retains_capacity() {
//         let mut extensions = ExtensionsInner::new();
//         extensions.insert(5i32);
//         extensions.insert(MyType(10));
//         extensions.insert(true);

//         assert_eq!(extensions.map.len(), 3);
//         let prev_capacity = extensions.map.capacity();
//         extensions.clear();

//         assert_eq!(
//             extensions.map.len(),
//             0,
//             "after clear(), extensions map should have length 0"
//         );
//         assert_eq!(
//             extensions.map.capacity(),
//             prev_capacity,
//             "after clear(), extensions map should retain prior capacity"
//         );
//     }

//     #[test]
//     fn clear_drops_elements() {
//         use std::sync::Arc;
//         struct DropMePlease(Arc<()>);
//         struct DropMeTooPlease(Arc<()>);

//         let mut extensions = ExtensionsInner::new();
//         let val1 = DropMePlease(Arc::new(()));
//         let val2 = DropMeTooPlease(Arc::new(()));

//         let val1_dropped = Arc::downgrade(&val1.0);
//         let val2_dropped = Arc::downgrade(&val2.0);
//         extensions.insert(val1);
//         extensions.insert(val2);

//         assert!(val1_dropped.upgrade().is_some());
//         assert!(val2_dropped.upgrade().is_some());

//         extensions.clear();
//         assert!(
//             val1_dropped.upgrade().is_none(),
//             "after clear(), val1 should be dropped"
//         );
//         assert!(
//             val2_dropped.upgrade().is_none(),
//             "after clear(), val2 should be dropped"
//         );
//     }
// }
