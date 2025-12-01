/// A primitive data type
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum DataType {
    Bool = 0,
    Double = 1,
    Int = 2,
    Float = 3,
    String = 4,
    Raw = 5,
    BoolArray = 16,
    DoubleArray = 17,
    IntArray = 18,
    FloatArray = 19,
    StringArray = 20,
}
impl DataType {
    pub const fn add_array(self) -> DataType {
        match self {
            Self::Bool => Self::BoolArray,
            Self::Double => Self::DoubleArray,
            Self::Int => Self::IntArray,
            Self::Float => Self::FloatArray,
            Self::String => Self::StringArray,
            _ => panic!(
                "attempted to call add_array for a type that doesn't have an array counterpart!"
            ),
        }
    }
}

/// A generic value that can hold any of the primitive types.
#[derive(Debug, Clone, PartialEq)]
#[repr(u8)]
pub enum GenericValue {
    Bool(bool) = 0,
    Double(f64) = 1,
    Int(i64) = 2,
    Float(f32) = 3,
    String(String) = 4,
    BoolArray(Vec<bool>) = 16,
    DoubleArray(Vec<f64>) = 17,
    IntArray(Vec<i64>) = 18,
    FloatArray(Vec<f32>) = 19,
    StringArray(Vec<String>) = 20,
}
impl GenericType for GenericValue {
    fn data_type(&self) -> DataType {
        match self {
            Self::Bool(_) => DataType::Bool,
            Self::Double(_) => DataType::Double,
            Self::Int(_) => DataType::Int,
            Self::Float(_) => DataType::Float,
            Self::String(_) => DataType::String,
            Self::BoolArray(_) => DataType::BoolArray,
            Self::DoubleArray(_) => DataType::DoubleArray,
            Self::IntArray(_) => DataType::IntArray,
            Self::FloatArray(_) => DataType::FloatArray,
            Self::StringArray(_) => DataType::StringArray,
        }
    }
    fn type_string(&self) -> &'static str {
        match self {
            Self::Bool(_) => "boolean",
            Self::Double(_) => "double",
            Self::Int(_) => "int",
            Self::Float(_) => "float",
            Self::String(_) => "string",
            Self::BoolArray(_) => "boolean[]",
            Self::DoubleArray(_) => "double[]",
            Self::IntArray(_) => "int[]",
            Self::FloatArray(_) => "float[]",
            Self::StringArray(_) => "string[]",
        }
    }
    fn serialize_rmp(&self, buf: &mut Vec<u8>) {
        match self {
            Self::Bool(v) => v.serialize_rmp(buf),
            Self::Double(v) => v.serialize_rmp(buf),
            Self::Int(v) => v.serialize_rmp(buf),
            Self::Float(v) => v.serialize_rmp(buf),
            Self::String(v) => v.serialize_rmp(buf),
            Self::BoolArray(v) => v.serialize_rmp(buf),
            Self::DoubleArray(v) => v.serialize_rmp(buf),
            Self::IntArray(v) => v.serialize_rmp(buf),
            Self::FloatArray(v) => v.serialize_rmp(buf),
            Self::StringArray(v) => v.serialize_rmp(buf),
        }
    }
}

/// A generic type of data that can be serialized and has a type known at runtime.
pub trait GenericType {
    fn data_type(&self) -> DataType;
    fn type_string(&self) -> &'static str;
    fn serialize_rmp(&self, buf: &mut Vec<u8>);
}

/// Data that, in addition to being able to be serialized, also has its to type known at compile-time.
pub trait ConcreteType: GenericType {
    fn data_type() -> DataType;
    fn type_string() -> &'static str;
}
impl<T: GenericType + ?Sized> GenericType for &T {
    fn data_type(&self) -> DataType {
        T::data_type(self)
    }
    fn type_string(&self) -> &'static str {
        T::type_string(self)
    }
    fn serialize_rmp(&self, buf: &mut Vec<u8>) {
        T::serialize_rmp(self, buf);
    }
}

macro_rules! impl_for_int {
    ($($int:ty)*) => {
        $(
            impl GenericType for $int {
                fn data_type(&self) -> DataType {
                    DataType::Int
                }
                fn type_string(&self) -> &'static str {
                    "int"
                }
                fn serialize_rmp(&self, buf: &mut Vec<u8>) {
                    let _ = rmp::encode::write_sint(buf, *self as i64);
                }
            }
            impl ConcreteType for $int {
                fn data_type() -> DataType {
                    DataType::Int
                }
                fn type_string() -> &'static str {
                    "int"
                }
            }
            impl From<$int> for GenericValue {
                fn from(value: $int) -> Self {
                    Self::Int(value as _)
                }
            }
            impl GenericType for Vec<$int> {
                fn data_type(&self) -> DataType {
                    DataType::IntArray
                }
                fn type_string(&self) -> &'static str {
                    "int[]"
                }
                fn serialize_rmp(&self, buf: &mut Vec<u8>) {
                    let _ = rmp::encode::write_array_len(buf, self.len() as _);
                    for elem in self {
                        let _ = rmp::encode::write_sint(buf, *elem as i64);
                    }
                }
            }
            impl ConcreteType for Vec<$int> {
                fn data_type() -> DataType {
                    DataType::IntArray
                }
                fn type_string() -> &'static str {
                    "int[]"
                }
            }
            impl From<Vec<$int>> for GenericValue {
                fn from(value: Vec<$int>) -> Self {
                    Self::IntArray(value.into_iter().map(|v| v as _).collect())
                }
            }
        )*
    };
}
impl_for_int!(i8 i16 i32 i64 isize u8 u16 u32 u64 usize);
macro_rules! impl_for_prim {
    ($($type:ty, $data:ident, $dataarr:ident, $string:expr, $encode:expr;)*) => {
        $(
            impl GenericType for $type {
                fn data_type(&self) -> DataType {
                    DataType::$data
                }
                fn type_string(&self) -> &'static str {
                    $string
                }
                fn serialize_rmp(&self, buf: &mut Vec<u8>) {
                    let _ = $encode(buf, self);
                }
            }
            impl ConcreteType for $type {
                fn data_type() -> DataType {
                    DataType::$data
                }
                fn type_string() -> &'static str {
                    $string
                }
            }
            impl From<$type> for GenericValue {
                fn from(value: $type) -> Self {
                    Self::$data(value)
                }
            }
            impl GenericType for Vec<$type> {
                fn data_type(&self) -> DataType {
                    DataType::$dataarr
                }
                fn type_string(&self) -> &'static str {
                    concat!($string, "[]")
                }
                fn serialize_rmp(&self, buf: &mut Vec<u8>) {
                    let _ = rmp::encode::write_array_len(buf, self.len() as _);
                    for elem in self {
                        let _ = $encode(buf, elem);
                    }
                }
            }
            impl ConcreteType for Vec<$type> {
                fn data_type() -> DataType {
                    DataType::$dataarr
                }
                fn type_string() -> &'static str {
                    concat!($string, "[]")
                }
            }
            impl From<Vec<$type>> for GenericValue {
                fn from(value: Vec<$type>) -> Self {
                    Self::$dataarr(value)
                }
            }
        )*
    };
}
impl_for_prim!(
    f32, Float, FloatArray, "float", write_f32;
    f64, Double, DoubleArray, "double", write_f64;
    bool, Bool, BoolArray, "boolean", write_bool;
    String, String, StringArray, "string", rmp::encode::write_str;
);
#[inline(always)]
fn write_f32(buf: &mut Vec<u8>, val: &f32) {
    let _ = rmp::encode::write_f32(buf, *val);
}
#[inline(always)]
fn write_f64(buf: &mut Vec<u8>, val: &f64) {
    let _ = rmp::encode::write_f64(buf, *val);
}
#[inline(always)]
fn write_bool(buf: &mut Vec<u8>, val: &bool) {
    let _ = rmp::encode::write_bool(buf, *val);
}
