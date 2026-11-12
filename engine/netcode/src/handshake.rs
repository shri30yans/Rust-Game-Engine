//! Handshake state machines that map hellos to welcomes or rejects (no I/O).

use crate::error::NetError;
use crate::input::InputPayload;
use crate::protocol::{ClientIndex, GameProtocol, Message, ProtocolVersion, RejectReason};

/// Client-side handshake progression.
#[derive(Debug, Clone)]
pub struct ClientHandshake {
    /// Game identity the client offered in [`Message::Hello`].
    pub game: GameProtocol,
    /// Opaque nonce from the opening hello.
    pub client_nonce: u64,
    connected: bool,
}

impl ClientHandshake {
    /// Starts a handshake and returns the opening [`Message::Hello`].
    #[must_use]
    pub fn start<I: InputPayload>(game: GameProtocol, client_nonce: u64) -> (Self, Message<I>) {
        let state = ClientHandshake {
            game,
            client_nonce,
            connected: false,
        };
        let hello = Message::Hello {
            version: ProtocolVersion::CURRENT,
            game,
            client_nonce,
        };
        (state, hello)
    }

    /// Processes the server's reply.
    ///
    /// # Errors
    /// Returns [`NetError::Decode`] when the message is not a terminal handshake reply.
    pub fn on_server_message<I: InputPayload>(&mut self, msg: Message<I>) -> Result<(), NetError> {
        match msg {
            Message::Welcome { .. } => {
                self.connected = true;
                Ok(())
            }
            Message::Reject { .. } => Ok(()),
            _ => Err(NetError::Decode(postcard::Error::DeserializeBadEncoding)),
        }
    }

    /// Whether the client received [`Message::Welcome`].
    #[must_use]
    pub const fn is_connected(&self) -> bool {
        self.connected
    }
}

/// Server-side handshake gate with a fixed client capacity.
#[derive(Debug, Clone)]
pub struct ServerHandshake {
    game: GameProtocol,
    capacity: u8,
    next_index: u8,
}

impl ServerHandshake {
    /// Creates a server handshake for `game` with at most `capacity` clients.
    #[must_use]
    pub const fn new(game: GameProtocol, capacity: u8) -> Self {
        ServerHandshake {
            game,
            capacity,
            next_index: 0,
        }
    }

    /// Handles a client [`Message::Hello`] and returns welcome or reject.
    #[must_use]
    pub fn on_hello<I: InputPayload>(
        &mut self,
        version: ProtocolVersion,
        game: GameProtocol,
        _client_nonce: u64,
    ) -> Message<I> {
        if version != ProtocolVersion::CURRENT {
            return Message::Reject {
                reason: RejectReason::VersionMismatch,
            };
        }
        if game.id != self.game.id || game.version != self.game.version {
            return Message::Reject {
                reason: RejectReason::WrongGame,
            };
        }
        if self.next_index >= self.capacity {
            return Message::Reject {
                reason: RejectReason::ServerFull,
            };
        }
        let client_index = ClientIndex::new(self.next_index);
        self.next_index = self.next_index.saturating_add(1);
        Message::Welcome {
            version: ProtocolVersion::CURRENT,
            client_index,
        }
    }

    /// Handles a top-level client message (only [`Message::Hello`] is acted on).
    #[must_use]
    pub fn on_client_message<I: InputPayload>(&mut self, msg: Message<I>) -> Option<Message<I>> {
        match msg {
            Message::Hello {
                version,
                game,
                client_nonce,
            } => Some(self.on_hello(version, game, client_nonce)),
            _ => None,
        }
    }
}
