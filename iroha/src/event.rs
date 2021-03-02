//! Iroha is a quite dynamic system so many events can happen.
//! This module contains descriptions of such an events and
//! utilitary Iroha Special Instructions to work with them.

use async_std::{
    future,
    sync::{Receiver, Sender},
};
use futures::{SinkExt, StreamExt};
use iroha_data_model::events::{prelude::*, SubscriptionRequest};
use iroha_http_server::web_socket::{WebSocketMessage, WebSocketStream};
use iroha_version::prelude::*;
use std::{fmt::Debug, time::Duration};

/// Type of `Sender<Event>` which should be used for channels of `Event` messages.
pub type EventsSender = Sender<Event>;
/// Type of `Receiver<Event>` which should be used for channels of `Event` messages.
pub type EventsReceiver = Receiver<Event>;

const TIMEOUT: Duration = Duration::from_millis(1000);

/// Consumer for Iroha `Event`(s).
/// Passes the events over the corresponding connection `stream` if they match the `filter`.
#[derive(Debug)]
pub struct Consumer {
    stream: WebSocketStream,
    filter: EventFilter,
}

impl Consumer {
    /// Constructs `Consumer`, which consumes `Event`s and forwards it through the `stream`.
    pub async fn new(mut stream: WebSocketStream) -> Result<Self, String> {
        if let WebSocketMessage::Text(message) = future::timeout(TIMEOUT, stream.next())
            .await
            .map_err(|e| format!("Read message timeout: {}", e))?
            .ok_or("Failed to read message: no message")?
            .map_err(|e| format!("Web Socket failure: {}", e))?
        {
            let request: SubscriptionRequest =
                VersionedSubscriptionRequest::from_versioned_json_str(&message)?
                    .as_v1()
                    .ok_or("Expected subscription request version 1.")?
                    .clone()
                    .into();
            let SubscriptionRequest(filter) = request;
            Ok(Consumer { stream, filter })
        } else {
            Err("Unexepcted message type".to_string())
        }
    }

    /// Forwards the `event` over the `stream` if it matches the `filter`.
    pub async fn consume(mut self, event: &Event) -> Result<Self, String> {
        if self.filter.apply(event) {
            let event = VersionedEvent::from(event.clone())
                .to_versioned_json_str()
                .map_err(|err| format!("Failed to serialize event: {}", err))?;
            future::timeout(TIMEOUT, self.stream.send(WebSocketMessage::Text(event)))
                .await
                .map_err(|e| format!("Read message timeout: {}", e))?
                .map_err(|e| format!("Failed to write message: {}", e))?;
            if let WebSocketMessage::Text(receipt) = future::timeout(TIMEOUT, self.stream.next())
                .await
                .map_err(|e| format!("Failed to read receipt: {}", e))?
                .ok_or("Failed to read receipt: no receipt")?
                .map_err(|e| format!("Web Socket failure: {}", e))?
            {
                let _receipt =
                    VersionedEventReceived::from_versioned_json_str(&receipt).map_err(|_| {
                        format!("Unexpected message, waited for receipt got: {}", receipt)
                    })?;
            } else {
                return Err("Unexepcted message type".to_string());
            }
        }
        Ok(self)
    }
}
