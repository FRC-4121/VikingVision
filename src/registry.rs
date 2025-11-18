use serde::de::*;
use std::any::TypeId;
use std::borrow::Cow;
use std::cell::UnsafeCell;
use std::collections::HashMap;
use std::fmt::{self, Debug, Formatter};
use std::marker::PhantomData;

pub type DeserializeFn<T> = fn(&mut dyn erased_serde::Deserializer) -> erased_serde::Result<T>;

thread_local! {
    static REGISTRIES: UnsafeCell<HashMap<TypeId, *const Registry<()>>> = UnsafeCell::default();
}

#[derive(Debug, Clone)]
pub struct Registry<T> {
    pub field: &'static str,
    pub lookup: HashMap<&'static str, DeserializeFn<T>>,
}
impl<T> Registry<T> {
    pub fn new(field: &'static str) -> Self {
        Self {
            field,
            lookup: HashMap::new(),
        }
    }
    /// Ascribe a type to this registry
    ///
    /// This is a no-op and only used to guide type inference
    #[inline(always)]
    pub fn ascribe(self, _: PhantomData<T>) -> Self {
        self
    }
}
impl<T: 'static> Registry<T> {
    /// Insert a registry into the dynamic scope and call a function with it available.
    ///
    /// This uses thread-local variables, so calls to other threads won't work.
    pub fn in_scope<R, F: FnOnce() -> R>(&self, f: F) -> R {
        let old = REGISTRIES.with(|r| unsafe {
            (*r.get()).insert(
                TypeId::of::<Self>(),
                self as *const Self as *const Registry<()>,
            )
        });
        struct DropGuard(TypeId, Option<*const Registry<()>>);
        impl Drop for DropGuard {
            fn drop(&mut self) {
                REGISTRIES.with(|r| unsafe {
                    let r = &mut *r.get();
                    if let Some(old) = self.1 {
                        r.insert(self.0, old);
                    } else {
                        r.remove(&self.0);
                    }
                })
            }
        }
        let _guard = DropGuard(TypeId::of::<Self>(), old);
        f()
    }
    /// Like [`Self::from_scope`], but doesn't panic without a registry.
    pub fn try_from_scope<R, F: FnOnce(&Self) -> R>(f: F) -> Option<R> {
        REGISTRIES
            .try_with(|r| unsafe {
                let r = (*r.get()).get(&TypeId::of::<Self>());
                r.map(|reg| f(&*(*reg as *const Self)))
            })
            .ok()
            .flatten()
    }
    /// Get a previously inserted [`Registry`] from within the closure passed to [`Self::in_scope`].
    ///
    /// This panics if a registry isn't available.
    pub fn from_scope<R, F: FnOnce(&Self) -> R>(f: F) -> R {
        Self::try_from_scope(f).expect(
            "A registry of the correct type must be in scope from a call to Registry::in_scope!",
        )
    }
}
impl<T: DefaultDiscriminant> Default for Registry<T> {
    fn default() -> Self {
        Self::new(T::default_discriminant())
    }
}

pub trait Register<T> {
    fn register(registry: &mut Registry<T>);
    fn registry_with_field(field: &'static str) -> Registry<T> {
        let mut registry = Registry::new(field);
        Self::register(&mut registry);
        registry
    }
}
pub trait RegisterExt<T> {
    fn registry() -> Registry<T>;
}
impl<T: DefaultDiscriminant, R: Register<T>> RegisterExt<T> for R {
    fn registry() -> Registry<T> {
        R::registry_with_field(T::default_discriminant())
    }
}

pub trait DefaultDiscriminant {
    fn default_discriminant() -> &'static str;
}
impl<T: DefaultDiscriminant + ?Sized> DefaultDiscriminant for Box<T> {
    fn default_discriminant() -> &'static str {
        T::default_discriminant()
    }
}

impl<'de, T> Visitor<'de> for &Registry<T> {
    type Value = T;
    fn expecting(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(formatter, "a {}", disqualified::ShortName::of::<T>())
    }
    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let tag = seq
            .next_element::<Cow<'de, str>>()?
            .ok_or_else(|| A::Error::invalid_length(0, &self))?;
        let deser = self
            .lookup
            .get(&*tag)
            .ok_or_else(|| A::Error::custom(format!("Unknown tag {tag:?}")))?;
        deser(&mut <dyn erased_serde::Deserializer>::erase(
            value::SeqAccessDeserializer::new(seq),
        ))
        .map_err(A::Error::custom)
    }
    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut kv = Vec::new();
        while let Some(key) = map.next_key::<Cow<'de, str>>()? {
            if key == self.field {
                let tag = map.next_value::<Cow<'de, str>>()?;
                let deser = self
                    .lookup
                    .get(&*tag)
                    .ok_or_else(|| A::Error::custom(format!("Unknown tag {tag:?}")))?;
                return deser(&mut <dyn erased_serde::Deserializer>::erase(
                    value::MapAccessDeserializer::new(PartialMapAccess {
                        value: None,
                        kv: kv.into_iter(),
                        remaining: map,
                    }),
                ))
                .map_err(A::Error::custom);
            } else {
                kv.push((key, map.next_value()?));
            }
        }
        Err(A::Error::missing_field(self.field))
    }
}
impl<'de, T> DeserializeSeed<'de> for &Registry<T> {
    type Value = T;
    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(self)
    }
}
struct PartialMapAccess<'de, A> {
    value: Option<crate::content::Content<'de>>,
    kv: std::vec::IntoIter<(Cow<'de, str>, crate::content::Content<'de>)>,
    remaining: A,
}
impl<'de, A: MapAccess<'de>> MapAccess<'de> for PartialMapAccess<'de, A> {
    type Error = A::Error;
    fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>, Self::Error>
    where
        K: DeserializeSeed<'de>,
    {
        if let Some((k, v)) = self.kv.next() {
            self.value = Some(v);
            seed.deserialize(value::CowStrDeserializer::new(k))
                .map(Some)
        } else {
            self.value = None;
            self.remaining.next_key_seed(seed)
        }
    }
    fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value, Self::Error>
    where
        V: DeserializeSeed<'de>,
    {
        if let Some(v) = self.value.take() {
            seed.deserialize(v.into_deserializer())
        } else if self.kv.len() == 0 {
            self.remaining.next_value_seed(seed)
        } else {
            panic!("Called next_value before next_key")
        }
    }
    fn next_entry_seed<K, V>(
        &mut self,
        kseed: K,
        vseed: V,
    ) -> Result<Option<(K::Value, V::Value)>, Self::Error>
    where
        K: DeserializeSeed<'de>,
        V: DeserializeSeed<'de>,
    {
        self.value = None;
        if let Some((k, v)) = self.kv.next() {
            Ok(Some((
                kseed.deserialize(value::CowStrDeserializer::new(k))?,
                vseed.deserialize(v.into_deserializer())?,
            )))
        } else {
            self.remaining.next_entry_seed(kseed, vseed)
        }
    }
}

#[macro_export]
macro_rules! impl_register {
    (; $($tag:expr => $this:ty),* $(,)?) => {};
    ([$dyntrait:ty] $([$($exp:tt)*])*; $($tag:expr => $this:ty),* $(,)?) => {
        $(
            impl $crate::registry::Register<Box<$dyntrait>> for $this {
                fn register(registry: &mut $crate::registry::Registry<Box<$dyntrait>>) {
                    registry.lookup.insert($tag, |deserializer| erased_serde::deserialize::<Self>(deserializer).map(|b| Box::new(b) as _));
                }
            }
        )*
        $crate::impl_register!($([$($exp)*])*; $($tag => $this),*);
    };
    ([gen <$($generics:ident),* $(,)?> $dyntrait:ty $(where $($wc:tt)*)?] $([$($exp:tt)*])*; $tag:expr => $this:ty $(, $tags:expr => $these:ty)* $(,)? $(; @remaining $($tags2:expr => $this2:ty)*)?) => {
        impl<$($generics),*> $crate::registry::Register<Box<$dyntrait>> for $this (where $($wc)*)? {
            fn register(registry: &mut $crate::registry::Registry<Box<$dyntrait>>) {
                registry.lookup.insert($tag, |deserializer| erased_serde::deserialize::<Self>(deserializer).map(|b| Box::new(b) as _));
            }
        }
        $crate::impl_register!([gen <$($generics),*> $dyntrait $(where $($wc)*)?] $([$($exp)*])*; $($tags => $these),*; @remaining $tag => $this);
    };
    (gen [$($ign:tt)*] $([$($exp:tt)*])*;; @remaining $($tags:expr => $these:ty)*) => {
        $crate::impl_register!($([$($exp)*])*; $($tags => $these),*);
    };
    (in $bundle:ty; $([$($exp:tt)*])*; $($tag:expr => $this:ty),* $(,)?) => {
        $crate::impl_register_bundle!($bundle: $($this),*);
        $crate::impl_register!($([$($exp)*])*; $($tag => $this),*);
    };
}

/// Create a "bundle" implementation for [`Register`] that calls the implementations for contained types
#[macro_export]
macro_rules! impl_register_bundle {
    ($bundle:ty: $($component:ty),* $(,)?) => {
        impl<T> $crate::registry::Register<T> for $bundle where $($component: $crate::registry::Register<T>),* {
            fn register(registry: &mut $crate::registry::Registry<T>) {
                $(
                    <$component>::register(registry);
                )*
            }
        }
    };
}

#[macro_export]
macro_rules! impl_deserialize_via_registry {
    (<$($generics:ident),* $(,)?> $self:ty $(where $($wc:tt)*)? $(as $field:expr)?) => {
        impl<'de, $($generics)*> serde::Deserialize<'de> for $self $(where $($wc)*)? {
            fn deserialize<D>(deserializer: D) -> Result<$self, D::Error> where D: serde::Deserializer<'de> {
                $crate::registry::Registry::<$self>::from_scope(|registry| serde::de::DeserializeSeed::deserialize(registry, deserializer))
            }
        }
        $(
            impl $crate::registry::DefaultDiscriminant for $self {
                fn default_discriminant() -> &'static str {
                    $field
                }
            }
        )?
    };
}
