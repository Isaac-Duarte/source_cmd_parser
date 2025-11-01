use std::{
    any::{Any, TypeId},
    collections::HashMap,
    future::Future,
    pin::Pin,
    sync::Arc,
};

use tokio::sync::RwLock;

use super::cooldowns::Cooldowns;

pub type StateHandle = Arc<RwLock<State>>;

pub trait StateValue: Any + Send + Sync {}

impl<T> StateValue for T where T: Any + Send + Sync {}

pub struct State {
    store: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
}

impl State {
    pub fn new() -> Self {
        Self {
            store: HashMap::new(),
        }
    }

    pub fn with_defaults() -> Self {
        let mut state = Self::new();
        state.set(Cooldowns::default());
        state
    }

    pub fn set<T>(&mut self, value: T)
    where
        T: StateValue,
    {
        self.store
            .insert(TypeId::of::<T>(), Box::new(value));
    }

    pub fn get<T>(&self) -> Option<&T>
    where
        T: StateValue,
    {
        self.store
            .get(&TypeId::of::<T>())
            .and_then(|data| data.downcast_ref::<T>())
    }

    pub fn get_mut<T>(&mut self) -> Option<&mut T>
    where
        T: StateValue,
    {
        self.store
            .get_mut(&TypeId::of::<T>())
            .and_then(|data| data.downcast_mut::<T>())
    }

    pub fn get_or_insert_with<T, F>(&mut self, constructor: F) -> &mut T
    where
        T: StateValue,
        F: FnOnce() -> T,
    {
        self.store
            .entry(TypeId::of::<T>())
            .or_insert_with(|| Box::new(constructor()))
            .downcast_mut::<T>()
            .expect("Type mismatch in state store")
    }

    pub fn take<T>(&mut self) -> Option<T>
    where
        T: StateValue,
    {
        self.store
            .remove(&TypeId::of::<T>())
            .and_then(|boxed| boxed.downcast::<T>().ok())
            .map(|boxed| *boxed)
    }

    pub fn contains<T>(&self) -> bool
    where
        T: StateValue,
    {
        self.store.contains_key(&TypeId::of::<T>())
    }
}

impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}

#[allow(dead_code)]
pub trait StateHandleExt {
    fn with_read<F, R>(&self, f: F) -> Pin<Box<dyn Future<Output = R> + Send>>
    where
        F: FnOnce(&State) -> R + Send + 'static,
        R: Send + 'static;

    fn with_write<F, R>(&self, f: F) -> Pin<Box<dyn Future<Output = R> + Send>>
    where
        F: FnOnce(&mut State) -> R + Send + 'static,
        R: Send + 'static;

    fn set<T>(&self, value: T) -> Pin<Box<dyn Future<Output = ()> + Send>>
    where
        T: StateValue + Send + 'static;

    fn get<T>(&self) -> Pin<Box<dyn Future<Output = Option<T>> + Send>>
    where
        T: StateValue + Clone + Send + 'static;

    fn update<T, F, R>(&self, f: F) -> Pin<Box<dyn Future<Output = Option<R>> + Send>>
    where
        T: StateValue + Send + 'static,
        F: FnOnce(&mut T) -> R + Send + 'static,
        R: Send + 'static;
}

impl StateHandleExt for StateHandle {
    fn with_read<F, R>(&self, f: F) -> Pin<Box<dyn Future<Output = R> + Send>>
    where
        F: FnOnce(&State) -> R + Send + 'static,
        R: Send + 'static,
    {
        let handle = Arc::clone(self);
        Box::pin(async move {
            let state = handle.read().await;
            f(&state)
        })
    }

    fn with_write<F, R>(&self, f: F) -> Pin<Box<dyn Future<Output = R> + Send>>
    where
        F: FnOnce(&mut State) -> R + Send + 'static,
        R: Send + 'static,
    {
        let handle = Arc::clone(self);
        Box::pin(async move {
            let mut state = handle.write().await;
            f(&mut state)
        })
    }

    fn set<T>(&self, value: T) -> Pin<Box<dyn Future<Output = ()> + Send>>
    where
        T: StateValue + Send + 'static,
    {
        let handle = Arc::clone(self);
        Box::pin(async move {
            let mut state = handle.write().await;
            state.set(value);
        })
    }

    fn get<T>(&self) -> Pin<Box<dyn Future<Output = Option<T>> + Send>>
    where
        T: StateValue + Clone + Send + 'static,
    {
        let handle = Arc::clone(self);
        Box::pin(async move {
            let state = handle.read().await;
            state.get::<T>().cloned()
        })
    }

    fn update<T, F, R>(&self, f: F) -> Pin<Box<dyn Future<Output = Option<R>> + Send>>
    where
        T: StateValue + Send + 'static,
        F: FnOnce(&mut T) -> R + Send + 'static,
        R: Send + 'static,
    {
        let handle = Arc::clone(self);
        Box::pin(async move {
            let mut state = handle.write().await;
            state.get_mut::<T>().map(f)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Debug, PartialEq, Eq)]
    struct Sample(&'static str);

    #[test]
    fn set_and_get_roundtrip() {
        let mut state = State::new();
        state.set::<usize>(7);
        assert_eq!(state.get::<usize>().copied(), Some(7));
    }

    #[test]
    fn take_removes_value() {
        let mut state = State::new();
        state.set::<Sample>(Sample("value"));
        let value = state.take::<Sample>().expect("value missing");
        assert_eq!(value, Sample("value"));
        assert!(!state.contains::<Sample>());
    }

    #[test]
    fn get_or_insert_returns_existing() {
        let mut state = State::new();
        state.set::<usize>(5);
        let existing = state.get_or_insert_with::<usize, _>(|| 10);
        assert_eq!(*existing, 5);
    }
}
