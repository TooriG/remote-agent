//! JSON conversions for recorded fixtures and tests.
pub use super::requests::{
    fixture_reply as reply, from_json as call, provider_response as response,
};
use super::*;
pub fn notification(method: &str, mut params: Value) -> Result<Notification, serde_json::Error> {
    if let Value::Object(fields) = &mut params {
        fields.insert("method".into(), method.into());
    }
    serde_json::from_value(params)
}

pub(super) fn typed_response<T: DeserializeOwned + Into<Body>>(
    line: &str,
) -> Result<Response, crate::message::RpcMessageError> {
    let reply = crate::message::RpcResponse::<T>::parse(line)?;
    Ok(Response::from_result(match reply.outcome {
        Ok(value) => Ok(value),
        Err(error) => Err(serde_json::from_str::<crate::error::RpcFailure>(
            error.get(),
        )?),
    }))
}
