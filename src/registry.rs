use serde::de::*;
use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt::{self, Debug, Display, Formatter};

pub type DeserializeFn<T> = fn(&mut dyn erased_serde::Deserializer) -> erased_serde::Result<T>;

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
}

pub trait Register<T> {
    fn register(registry: &mut Registry<T>);
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
    value: Option<serde_content::Value<'de>>,
    kv: std::vec::IntoIter<(Cow<'de, str>, serde_content::Value<'de>)>,
    remaining: A,
}
impl<'de, A: MapAccess<'de>> MapAccess<'de> for PartialMapAccess<'de, A> {
    type Error = PartialMapAccessError<A::Error>;
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
            self.remaining
                .next_key_seed(seed)
                .map_err(PartialMapAccessError::Underlying)
        }
    }
    fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value, Self::Error>
    where
        V: DeserializeSeed<'de>,
    {
        if let Some(v) = self.value.take() {
            seed.deserialize(v.into_deserializer())
                .map_err(PartialMapAccessError::Content)
        } else if self.kv.len() == 0 {
            self.remaining
                .next_value_seed(seed)
                .map_err(PartialMapAccessError::Underlying)
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
                vseed
                    .deserialize(v.into_deserializer())
                    .map_err(PartialMapAccessError::Content)?,
            )))
        } else {
            self.remaining
                .next_entry_seed(kseed, vseed)
                .map_err(PartialMapAccessError::Underlying)
        }
    }
}
#[derive(Clone)]
enum PartialMapAccessError<E> {
    Underlying(E),
    Content(serde_content::Error),
}
impl<E: Debug> Debug for PartialMapAccessError<E> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Underlying(e) => e.fmt(f),
            Self::Content(e) => Debug::fmt(e, f),
        }
    }
}
impl<E: Display> Display for PartialMapAccessError<E> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Underlying(e) => e.fmt(f),
            Self::Content(e) => Display::fmt(e, f),
        }
    }
}
impl<E: StdError> StdError for PartialMapAccessError<E> {}
impl<E: Error> Error for PartialMapAccessError<E> {
    fn custom<T>(msg: T) -> Self
    where
        T: fmt::Display,
    {
        Self::Underlying(E::custom(msg))
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
    ([gen <$($generics:tt),* $(,)?> $dyntrait:ty $(where $($wc:tt)*)?] $([$($exp:tt)*])*; $tag:expr => $this:ty $(, $tags:expr => $these:ty)* $(,)? $(; @remaining $($tags2:expr => $this2:ty)*)?) => {
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
