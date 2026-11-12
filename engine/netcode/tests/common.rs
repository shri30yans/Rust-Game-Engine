#![allow(dead_code, missing_docs)]

use engine_netcode::{InputPayload, NetError};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestInput {
    pub move_x: i8,
    pub buttons: u8,
}

impl InputPayload for TestInput {
    const MAX_BYTES: usize = 16;

    fn validate(&self) -> Result<(), NetError> {
        if self.buttons & 0x80 != 0 {
            return Err(NetError::InvalidLinkConditions("reserved button bit set"));
        }
        Ok(())
    }
}
