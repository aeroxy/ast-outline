use crate::error::Error;

pub struct Client;

impl Client {
    pub fn connect() -> Result<Self, Error> {
        Ok(Self)
    }
}
