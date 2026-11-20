use crate::decode::Decode;
use crate::encode::{Encode, IsNull};
use crate::error::BoxDynError;
use crate::types::Type;
use crate::{PgArgumentBuffer, PgHasArrayType, PgTypeInfo, PgValueFormat, PgValueRef, Postgres};

impl Type<Postgres> for bool {
    fn type_info() -> PgTypeInfo {
        PgTypeInfo::BOOL
    }

    // machina: a flag stored as an integer reads as a bool, as it does in SQLite
    fn compatible(ty: &PgTypeInfo) -> bool {
        *ty == PgTypeInfo::BOOL || super::lenient::is_int(ty)
    }
}

impl PgHasArrayType for bool {
    fn array_type_info() -> PgTypeInfo {
        PgTypeInfo::BOOL_ARRAY
    }
}

impl Encode<'_, Postgres> for bool {
    fn encode_by_ref(&self, buf: &mut PgArgumentBuffer) -> Result<IsNull, BoxDynError> {
        buf.push(*self as u8);

        Ok(IsNull::No)
    }
}

impl Decode<'_, Postgres> for bool {
    fn decode(value: PgValueRef<'_>) -> Result<Self, BoxDynError> {
        if value.format() == PgValueFormat::Binary && super::lenient::is_int(&value.type_info) {
            return Ok(<i64 as Decode<Postgres>>::decode(value)? != 0);
        }
        Ok(match value.format() {
            PgValueFormat::Binary => value.as_bytes()?[0] != 0,

            PgValueFormat::Text => match value.as_str()? {
                "t" => true,
                "f" => false,

                s => {
                    return Err(format!("unexpected value {s:?} for boolean").into());
                }
            },
        })
    }
}
