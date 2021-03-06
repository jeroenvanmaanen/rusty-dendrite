use anyhow::{Error,Result};
use bytes::Bytes;
use log::{debug};
use prost::Message;
use tokio::sync::mpsc;
use tonic::{Request, Response, Status};
use crate::axon_utils::{AxonServerHandle, CommandSink, QuerySink, init_command_sender, query_events};
use crate::grpc_example::greeter_service_server::GreeterService;
use crate::grpc_example::{Acknowledgement, Empty, GreetedEvent, Greeting, GreetCommand, RecordCommand, StopCommand, SearchQuery, SearchResponse};

#[derive(Debug)]
pub struct GreeterServer {
    pub axon_server_handle: AxonServerHandle,
}

#[tonic::async_trait]
impl GreeterService for GreeterServer {
    async fn greet(
        &self,
        request: Request<Greeting>,
    ) -> Result<Response<Acknowledgement>, Status> {
        debug!("Got a greet request: {:?}", request);
        let inner_request = request.into_inner();
        let result_message = inner_request.message.clone();

        let command = GreetCommand {
            aggregate_identifier: "xxx".to_string(),
            message: Some(inner_request),
        };

        if let Some(serialized) = self.axon_server_handle.send_command("GreetCommand", Box::new(&command)).await
            .map_err(to_status)?
        {
            let reply_from_command_handler = Message::decode(Bytes::from(serialized.data)).map_err(decode_error_to_status)?;
            debug!("Reply from command handler: {:?}", reply_from_command_handler);
            return Ok(Response::new(reply_from_command_handler));
        }

        let default_reply = Acknowledgement {
            message: format!("Hello {}!", result_message).into(),
        };

        Ok(Response::new(default_reply))
    }

    async fn record(
        &self,
        request: Request<Empty>,
    ) -> Result<Response<Empty>, Status> {
        debug!("Got a record request: {:?}", request);

        let command = RecordCommand {
            aggregate_identifier: "xxx".to_string(),
        };

        self.axon_server_handle.send_command("RecordCommand", Box::new(&command)).await.map_err(to_status)?;

        let reply = Empty { };

        Ok(Response::new(reply))
    }

    async fn stop(
        &self,
        request: Request<Empty>,
    ) -> Result<Response<Empty>, Status> {
        debug!("Got a stop request: {:?}", request);

        let command = StopCommand {
            aggregate_identifier: "xxx".to_string(),
        };

        self.axon_server_handle.send_command("StopCommand", Box::new(&command)).await.map_err(to_status)?;

        let reply = Empty { };

        Ok(Response::new(reply))
    }

    type GreetingsStream = mpsc::Receiver<Result<Greeting, Status>>;

    async fn greetings(&self, _request: Request<Empty>) -> Result<Response<Self::GreetingsStream>, Status> {
        let events = query_events(&self.axon_server_handle, "xxx").await.map_err(to_status)?;
        let (mut tx, rx) = mpsc::channel(4);

        tokio::spawn(async move {
            for event in &events[..] {
                let event = event.clone();
                if let Some(payload) = event.payload {
                    if payload.r#type == "GreetedEvent" {
                        let greeted_event_message = GreetedEvent::decode(Bytes::from(payload.data)).ok().map(|e| e.message);
                        if let Some(greeting) = greeted_event_message.flatten() {
                            debug!("Greeting: {:?}", greeting);
                            tx.send(Ok(greeting)).await.ok();
                        }
                    }
                }
            }
            let greeting = Greeting {
                message: "End of stream -oo-".to_string(),
            };
            debug!("End of stream: {:?}", greeting);
            tx.send(Ok(greeting)).await.ok();
        });

        Ok(Response::new(rx))
    }

    type SearchStream = mpsc::Receiver<Result<Greeting, Status>>;

    async fn search(&self, request: Request<SearchQuery>) -> Result<Response<Self::SearchStream>, Status> {
        let (mut tx, rx) = mpsc::channel(4);
        let query = request.into_inner();
        let query_response = self.axon_server_handle.send_query("SearchQuery", Box::new(&query)).await.map_err(to_status)?;

        tokio::spawn(async move {
            for serialized_object in query_response {
                if let Ok(search_response) = SearchResponse::decode(Bytes::from(serialized_object.data)) {
                    debug!("Search response: {:?}", search_response);
                    for greeting in search_response.greetings {
                        debug!("Greeting: {:?}", greeting);
                        tx.send(Ok(greeting)).await.ok();
                    }
                }
                debug!("Next!");
            }
            debug!("Done!")
        });

        Ok(Response::new(rx))
    }
}

pub async fn init() -> Result<GreeterServer> {
    init_command_sender().await.map(|command_sink| {GreeterServer{ axon_server_handle: command_sink }})
}

fn to_status(e: Error) -> Status {
    Status::unknown(e.to_string())
}

fn decode_error_to_status(e: prost::DecodeError) -> Status {
    Status::unknown(e.to_string())
}
