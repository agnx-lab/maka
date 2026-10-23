/*
 * Licensed to the Apache Software Foundation (ASF) under one
 * or more contributor license agreements.  See the NOTICE file
 * distributed with this work for additional information
 * regarding copyright ownership.  The ASF licenses this file
 * to you under the Apache License, Version 2.0 (the
 * "License"); you may not use this file except in compliance
 * with the License.  You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing,
 * software distributed under the License is distributed on an
 * "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
 * KIND, either express or implied.  See the License for the
 * specific language governing permissions and limitations
 * under the License.
 */

use rmcp::{
    RoleClient,
    service::{RxJsonRpcMessage, TxJsonRpcMessage},
    transport::Transport,
};
use tokio::sync::watch;

/// Keep a cleanup receipt even when rmcp drops a transport during initialization.
/// Dropping a future is not evidence that its HTTP worker has stopped.
pub(crate) struct Owned<T: Transport<RoleClient> + 'static> {
    transport: Option<T>,
    done: watch::Sender<Option<Result<(), String>>>,
}
pub(crate) type Closed = watch::Receiver<Option<Result<(), String>>>;
pub(crate) fn track<T: Transport<RoleClient> + 'static>(transport: T) -> (Owned<T>, Closed) {
    let (done, closed) = watch::channel(None);
    (
        Owned {
            transport: Some(transport),
            done,
        },
        closed,
    )
}
pub(crate) async fn closed(mut done: Closed) -> Result<(), String> {
    loop {
        if let Some(result) = done.borrow_and_update().clone() {
            return result;
        }
        done.changed()
            .await
            .map_err(|_| "MCP transport cleanup disappeared".to_owned())?;
    }
}
impl<T: Transport<RoleClient> + 'static> Transport<RoleClient> for Owned<T> {
    type Error = T::Error;
    fn send(
        &mut self,
        message: TxJsonRpcMessage<RoleClient>,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'static {
        self.transport
            .as_mut()
            .expect("open transport")
            .send(message)
    }
    async fn receive(&mut self) -> Option<RxJsonRpcMessage<RoleClient>> {
        self.transport.as_mut()?.receive().await
    }
    async fn close(&mut self) -> Result<(), Self::Error> {
        if let Some(mut transport) = self.transport.take() {
            let result = transport.close().await;
            self.done.send_replace(Some(
                result
                    .as_ref()
                    .map(|_| ())
                    .map_err(|_| "MCP transport cleanup failed".into()),
            ));
            result
        } else {
            Ok(())
        }
    }
}
impl<T: Transport<RoleClient> + 'static> Drop for Owned<T> {
    fn drop(&mut self) {
        if let Some(mut transport) = self.transport.take() {
            let done = self.done.clone();
            tokio::spawn(async move {
                let result = transport
                    .close()
                    .await
                    .map_err(|_| "MCP transport cleanup failed".into());
                done.send_replace(Some(result));
            });
        }
    }
}
