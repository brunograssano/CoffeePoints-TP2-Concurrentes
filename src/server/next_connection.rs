use std::{
    collections::{HashMap, HashSet},
    sync::{
        mpsc::{Receiver, RecvTimeoutError},
        Arc, Mutex,
    },
    time::Duration,
};

use async_std::task;
use lib::{
    connection_protocol::{ConnectionProtocol, TcpConnection},
    serializer::serialize,
};
use log::{error, info};

use crate::{
    address_resolver::id_to_address,
    connection_status::ConnectionStatus,
    constants::{
        INITIAL_WAIT_IN_MS_FOR_CONNECTION_ATTEMPT, MAX_WAIT_IN_MS_FOR_CONNECTION_ATTEMPT,
        TO_NEXT_CONN_CHANNEL_TIMEOUT_IN_MS,
    },
    errors::ServerError,
    server_messages::{
        create_new_connection_message, create_token_message, Diff, ServerMessage,
        ServerMessageType, TokenData,
    },
};

use self::sync::sleep;

mod sync {
    use std::thread;
    use std::time::Duration;

    #[cfg(not(test))]
    pub(crate) fn sleep(d: Duration) {
        thread::sleep(d);
    }

    #[cfg(test)]
    pub(crate) fn sleep(_: Duration) {
        thread::yield_now();
    }
}

pub struct NextConnection {
    id: usize,
    peer_count: usize,
    next_conn_receiver: Receiver<ServerMessage>,
    connection_status: Arc<Mutex<ConnectionStatus>>,
    connection: Option<Box<dyn ConnectionProtocol>>,
    initial_connection: bool,
    next_id: usize,
    last_token: Option<ServerMessage>,
}

impl NextConnection {
    pub fn new(
        id: usize,
        peer_count: usize,
        next_conn_receiver: Receiver<ServerMessage>,
        connection_status: Arc<Mutex<ConnectionStatus>>,
    ) -> NextConnection {
        let mut initial_connection = false;
        if id == 0 {
            initial_connection = true;
        }
        NextConnection {
            id,
            peer_count,
            next_conn_receiver,
            connection_status,
            connection: None,
            initial_connection,
            next_id: id,
            last_token: None,
        }
    }

    fn attempt_connections(&mut self, start: usize, stop: usize) -> Result<(), ServerError> {
        for id in start..stop {
            let result = TcpConnection::new_client_connection(id_to_address(id));
            if let Ok(connection) = result {
                self.next_id = id;
                self.connection = Some(Box::new(connection));
                self.connection_status.lock()?.set_next_online();
                let message = create_new_connection_message(self.id);
                if self.send_message(message).is_err() {
                    continue;
                }
                return Ok(());
            }
        }
        Err(ServerError::ConnectionLost)
    }

    fn connect_to_next(&mut self) -> Result<(), ServerError> {
        let peer_count = self.peer_count;
        let my_id = self.id;
        if self.attempt_connections(my_id + 1, peer_count).is_ok() {
            return Ok(());
        }
        if self.attempt_connections(0, my_id).is_ok() {
            return Ok(());
        }
        if my_id == 0 && self.initial_connection {
            self.initial_connection = false;
            return self.attempt_connections(0, 1);
        }
        self.connection_status.lock()?.set_next_offline();
        return Err(ServerError::ConnectionLost);
    }

    fn try_to_connect_wait_if_offline(&mut self) {
        let mut wait = INITIAL_WAIT_IN_MS_FOR_CONNECTION_ATTEMPT;
        loop {
            if self.connect_to_next().is_ok() {
                return;
            }
            sleep(Duration::from_millis(wait));
            wait *= 2;
            if wait >= MAX_WAIT_IN_MS_FOR_CONNECTION_ATTEMPT {
                wait = INITIAL_WAIT_IN_MS_FOR_CONNECTION_ATTEMPT;
            }
        }
    }

    pub fn handle_message_to_next(&mut self) -> Result<(), ServerError> {
        let timeout = Duration::from_millis(TO_NEXT_CONN_CHANNEL_TIMEOUT_IN_MS);
        self.try_to_connect_wait_if_offline();
        if self.id == 0 {
            if self.send_message(create_token_message(self.id)).is_err() {
                error!("Failed to send initial token");
                return Err(ServerError::ConnectionLost);
            }
            info!("Sent initial token to {}", self.next_id);
        }
        loop {
            if !self.connection_status.lock()?.is_online() {
                self.try_to_connect_wait_if_offline();
            }
            let result = self.next_conn_receiver.recv_timeout(timeout);
            if let Err(e) = result {
                match e {
                    RecvTimeoutError::Timeout => {
                        // Cubre el caso en que la red no quedo propiamente formada.
                        // Nos damos cuenta cuando no estamos escuchando mensajes de nadie (prev offline)
                        // pero nosotros nos creemos conectados.
                        // Ej. Red con nodos 0, 1, 2, 3. 2 esta offline y se logra conectar con 0 (justo con el 3 no pudo)
                        // 1 cierra la conexion con 3. La red quedo con un nodo apuntando al equivocado.
                        // Al detectar que no recibimos mensajes y no tenemos intenamos unirnos nuevamente.
                        // Con algo mas de logica podemos manejar particiones...
                        let mut connected = self.connection_status.lock()?;
                        if !connected.is_prev_online() {
                            connected.set_next_offline();
                            continue;
                        }
                    }
                    RecvTimeoutError::Disconnected => {
                        error!("[TO NEXT CONNECTION] Channel error, stopping...");
                        return Err(ServerError::ChannelError);
                    }
                }
            }
            let mut message = result.unwrap();
            match &mut message.message_type {
                ServerMessageType::NewConnection(_) => {
                    // no lo pude haber enviado yo, porque ya se habria manejado, quedan solo los otros casos

                    // todo, se puede mover al previous
                    // si no lo envie yo, pero estoy entre los que ya vieron el mensaje, lo descarto, esta circulando cuando ya dio vuelta

                    // si la nueva conexion esta entre yo y al que estoy conectado
                    // aviso al que estoy conectado con close connection
                    // me conecto al nuevo (o reintentamos con todos? reset general)
                    // le envio a la nueva conexion los datos, se agregan al diff

                    // si no esta en el medio, lo paso al siguiente y me agrego a la lista de por quien paso
                }
                ServerMessageType::Token(_) => {
                    // enviar el token al siguiente
                    // si tenemos cambios de una perdida anterior donde justo teniamos el token agregarlos y limpiarlo

                    // si falla reintentar conectarnos con el/los siguiente/s
                    // si fallan todas las reconexiones, perdimos la conexion y el token no es valido
                    // guardar los cambios hechos en otro lugar (solo las sumas) para appendearlos al proximo token cuando recuperemos la conexion
                    // hacemos continue, reintentamos hasta poder
                    // si perdimos la conexion marcamos que ya no tenemos el token

                    // si no fallan todas las reconexiones (ej logramos conectarnos al siguiente del siguiente)
                    // le mandamos el token, no se perdio

                    // marcamos en un mutex que ya no tenemos el token

                    self.last_token = Some(message.clone());
                    _ = self.send_message(message);
                }
                ServerMessageType::LostConnection(_) => {
                    // si el que perdio la conexion es al que apuntamos
                    // SOLO si es al que apuntamos, que nos llegue este mensaje es que se perdio el token
                    // (llego al final de la carrera - no estaba el token circulando porque se perdio)
                    // nos conectamos con el siguiente y mandarle mensaje token guardado

                    // mas que verlo como un lost connection verlo como un maybe we lost the token, circula por la red en forma de carrera (si esta el token)
                }
                _ => {}
            }
        }
    }

    fn send_message(&mut self, message: ServerMessage) -> Result<(), ServerError> {
        let message = serialize(&message)?;
        if let Some(connection) = self.connection.as_mut() {
            if task::block_on(connection.send(&message[..])).is_err() {
                self.connection = None;
                self.connection_status.lock()?.set_next_offline();
                return Err(ServerError::ConnectionLost);
            }
            return Ok(());
        }
        Err(ServerError::ConnectionLost)
    }
}
