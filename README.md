# TP2 - CoffeeGPT

El presente trabajo práctico tiene como objetivo implementar aplicaciones en Rust que modelen un sistema de puntos para fidelización de los clientes. Los clientes podrán sumar puntos por cada compra para canjearlos por cafés gratuitos.

Estas aplicaciones deben de trabajar en ambientes distribuidos susceptibles a fallas debido a perdida de conexión.

## Integrantes

| Nombre                                                        | Padrón |
| ------------------------------------------------------------- | ------ |
| [Grassano, Bruno](https://github.com/brunograssano)           | 103855 |
| [Roussilian, Juan Cruz](https://github.com/juan-roussilian)   | 104269 |
| [Stancanelli, Guillermo](https://github.com/guillermo-st)     | 104244 |

## Ejecución

La aplicación puede ser ejecutada a través de `cargo` con:

```
$ cargo run --bin [NOMBRE-APP] [ARGUMENTOS]
```

* Donde `[NOMBRE-APP]` puede ser `server` o `coffee_maker`
* Los valores de `[ARGUMENTOS]` dependen de la aplicación que se quiere ejecutar.
    * En el caso del server son `[ID] [TOTAL-SERVIDORES]` donde `[ID]` es el id del servidor (se debe de empezar con 0) y `[TOTAL-SERVIDORES]` la cantidad total de servidores que puede tener la red. Siempre se debe de iniciar el servidor 0 para que comience a funcionar correctamente.
    * En el caso de la cafetera `[IP:PORT] [FILE]` donde `[IP:PORT]` tiene la ip y puerto del servidor al que se va a conectar la cafetera y `[FILE]` el nombre del archivo. El nombre del archivo es opcional, si no se incluye se lee el ubicado en `tests/orders.csv` (definido por la constante `DEFAULT_ORDERS_FILE`)
* Se puede cambiar el nivel de log con la variable de entorno `RUST_LOG`. Algunos valores posibles son `error`, `info`, y `debug`

De forma completa quedaría:
```
$ RUST_LOG=info cargo run --bin server 0 5
$ RUST_LOG=info cargo run --bin coffee_maker 127.0.0.1:20000 tests/orders.csv
```

### Tests

Se proveen distintos casos de prueba de la aplicación. Se pueden ejecutar con:
```
$ cargo test
```

Algunas pruebas destacadas son:

### Dependencias y binarios
El trabajo práctico está dividido en las siguientes partes:
* Un binario para las cafeteras, `coffee_maker`
* Un binario para los servidores, `server`
* Una biblioteca con funcionalidades comunes a ambos binarios, `lib`


La aplicación tiene las siguientes dependencias:

* `rand` para generar números pseudoaleatorios, es usado para determinar el éxito de los pedidos.
* `actix` y `actix-rt` para el manejo de actores.
* `log` y `simple_logger` para tener la interfaz de los logs *(error!(), info!(), debug!())* y una implementación que imprime los mensajes.
* `async-std` para el manejo de tareas asincrónicas
* `async-trait` para poder definir interfaces con métodos *async*
* `bincode` y `serde` para serializar y deserializar a bytes los mensajes enviados.


## Diseño e implementación

### Arquitectura

La arquitectura del trabajo es de la siguiente forma:
![Arquitectura del trabajo](docs/arquitectura.png)

* Se tienen múltiples servidores locales que replican la base de datos de los puntos y están conectados entre sí
* Cada servidor local puede manejar múltiples conexiones de cafeteras

### Cafetera

Empezamos por la cafetera, la aplicación de la cafetera simula ser la máquina que hace el café en cada pedido. Estos pedidos son leídos de un archivo.

#### Formato del archivo

La cafetera para procesar los pedidos debe de leerlos de un archivo CSV que sigue el siguiente formato `OPERACION,COSTO/BENEFICIO,NRO CUENTA`. Donde:
* `OPERACION` es el tipo de pedido, puede ser de `CASH` para sumar puntos o `POINTS` para restar puntos. 
* `COSTO/BENEFICIO` es la cantidad que se va a sumar o restar de puntos. Es un número positivo
* `NRO CUENTA` es el id numérico positivo de la cuenta que realiza la operación.

Por ejemplo:


```
CASH,200,4
POINTS,200,2
POINTS,200,11
CASH,200,12
...
```

En caso de no respetarse el formato en una línea, se salteara e intentara leer la siguiente, siempre y cuando el archivo tenga un formato válido de UTF-8. Por ejemplo

```
CASH,200,4,442 <--- Falla la lectura y reintenta
POINTasdS,200,2 <--- Falla la lectura y reintenta
POINTS,200,-11 <--- Falla la lectura y reintenta
CASH,200,12 <--- Lee y parsea correctamente
...
```

#### Modelo

El modelo de la cafetera es el siguiente:
![Modelo de la cafetera](docs/modelo-cafetera.png)

En el diagrama podemos ver que la cafetera se puede dividir en dos partes que se comunican mediante mensajes, el lector de ordenes `OrdersReader` y la lógica del negocio en `CoffeeMaker`. Estas dos entidades están modeladas como actores.
* `OrdersReader` realiza la lectura y parseo del archivo CSV línea por línea a pedido de las cafeteras. Una vez realizada la lectura le responde a la cafetera con el pedido que tiene que realizar. Si ocurre un error en la lectura se envía un mensaje a sí mismo para que reintente y lea otra línea para esa misma cafetera.
* `CoffeeMaker` es el otro actor del modelo. Este actor realiza los pedidos de suma y resta. Cada uno tarda el tiempo definido en la constante `PROCESS_ORDER_TIME_IN_MS`.
    * Para saber si los pedidos fueron exitosos o no se separó la funcionalidad con el trait `Randomizer`. La probabilidad de éxito se define en la constante `SUCCESS_CHANCE`. Este trait adicionalmente permite manejar la parte pseudoaleatoria en los tests al usar mocks.
    * Para la comunicación con el servidor local se creó el cliente `LocalServerClient`. Este cliente se encarga de realizar y mantener la conexión.
    * `Protocol` es una interfaz para no acoplar la conexión a un protocolo de transporte en particular. La cafetera se conecta mediante TCP con el servidor local.
    * Si bien en el diagrama aparece como que hay una sola cafetera, puede configurarse mediante la constante `DISPENSERS` para que haya múltiples actores de este tipo. *Esto es para reducir la cantidad de aplicaciones a levantar.*

#### Actores y mensajes

En el siguiente diagrama se puede ver la comunicación entre los actores mencionados previamente.

![Comunicación entre los actores](docs/mensajes-cafetera.png)

1. El ciclo empieza una vez que `main` envía el mensaje `OpenFile` con las direcciones de las cafeteras a `OrdersReader`. El lector se va a guardar las direcciones y abrir el archivo.
2. Si se logra abrir exitosamente se les notifica a los actores de `CoffeeMaker` que se abrió con `OpenedFile`
3. Las cafeteras responden con el mensaje de `ReadAnOrder` para que el lector lea.
4. El lector le responde a cada cafetera que pedido tiene que atender en `ProcessOrder`
5. La cafetera procesa el pedido y vuelve a pedir otra orden.
6. Se repiten los pasos 4 y 5 hasta que se termine el archivo.

#### Comunicación con Servidor Local

Como ya se mencionó antes, para la comunicación cafetera-servidor local optamos por usar el protocolo de transporte TCP. Optamos por este protocolo debido a que garantiza que los datos serán entregados al servidor sin errores, en orden, y que la conexión con el servidor está activa.
La alternativa, UDP no garantiza nada de lo anterior, por lo que implicaba un desarrollo adicional para asegurar las propiedades mencionadas, principalmente los ACK y orden. 

Sin embargo, en la implementación se deja la libertad de intercambiar el protocolo empleado, ya que se tiene la interfaz `ConnectionProtocol`.

Pasando a los mensajes usados, se buscó tener un formato bien definido que sea independiente del tipo de pedido. Para eso definimos los campos comunes y se llegó a lo siguiente:

```rust
pub struct CoffeeMakerRequest {
    pub message_type: MessageType,
    pub account_id: usize,
    pub points: usize,
}

pub struct CoffeeMakerResponse {
    pub message_type: MessageType,
    pub status: ResponseStatus,
}
```

* `MessageType` y `ResponseStatus` son *enums* que tienen las distintas acciones/resultados.
* Los *structs* son serializados y deserializados mediante el crate `bincode` y `serde`.
* A los bytes enviados se le agrega al final el byte `;` para leer hasta ese punto.



### Servidor local

Cada sucursal de la cadena de café *CoffeeGPT* cuenta con su propio servidor local. 
Estos servidores locales tienen las conexiones con las cafeteras, las cuentas de los clientes, y controlan el acceso a los datos entre sí.

#### Comunicación

Para la comunicación entre los servidores elegimos usar el algoritmo *Token Ring* debido a que se resuelve la comunicación de forma sencilla. 
Cada servidor envía mensajes al siguiente y recibe del anterior en el anillo. Se puede ver en el diagrama de la arquitectura.

Al usar este modelo tenemos N conexiones (donde N es la cantidad de servidores), 
por lo que se vuelve una opción viable estar manteniendo esas conexiones en TCP, y de esta forma resolver el problema de asegurar que lleguen los mensajes. *Nota: Nuevamente, veremos que está la interfaz de ConnectionProtocol, por lo que se puede intercambiar.*

Pasamos ahora a ver los diferentes mensajes que pueden estar circulando por la red.

```rust
pub struct ServerMessage {
    pub message_type: ServerMessageType, // El tipo de mensaje
    pub sender_id: usize,                // Quien envio el mensaje
    pub passed_by: HashSet<usize>,       // Por quien paso el mensaje, si ya estoy en esta lista se descarta
}
```

##### Mensaje New Connection
El mensaje de `NewConnection` es el usado para indicar que hay una nueva conexión en la red. 
Se lanza al inicio cuando se levanta la red y cuando se quiere reconectar un servidor que estaba caído.

Este mensaje incluye los siguientes datos:

```rust
pub struct Diff {
    pub last_update: u128,              // Timestamp de la más reciente actualización que se tiene en la base
    pub changes: Vec<UpdatedAccount>,   // Cuentas actualizadas en base a la actualización
}
```

Veamos el funcionamiento con unos ejemplos.


En este diagrama podemos ver el comportamiento cuando se está levantando una red con 3 servidores.

![Comienzo de red](docs/inicio-conexion.png)
1. En este paso se levantó al servidor 0. Intento conectarse con 1 y 2 pero no lo logro, por lo que se conecta consigo mismo para que empiece a circular el token. Esto solo puede pasar al comienzo y con 0.
2. Se levanta el servidor 1. 
    1. Este intenta conectarse con 2 y no pudo, se conecta con 0. 
    2. Le envía el mensaje de NewConnection con fecha de última actualización 0 (no tiene nada guardado). 
    3. 0 al recibir el mensaje establece conexión con 1 y luego cierra con `CloseConnection` su propia conexión con 0. El cierre lo hace luego de establecida la conexión con 1 en caso de que se haya caído.
    4. El token está circulando entre estos dos nodos ahora.
3. El proceso se repite, con la diferencia que 0 pasa el mensaje a 1 debido a que 2 no está entre el 0 y 1.
4. La red luego de levantados los servidores.

Este otro ejemplo muestra el comportamiento cuando se tiene una red con 4 servidores y uno estaba caído.

![Se vuelve a conectar un servidor caído](docs/nueva-conexion-servidor-caido.png)
1. Estado inicial, la red formada del lado izquierdo y el nodo 2 sin conexión. El nodo 2 se encuentra en un *exponential backoff* intentando conectarse a la red. (Reintenta cada cierto tiempo conectarse, si falla duplica el tiempo para reintentar. Tiene un límite de `MAX_WAIT_IN_MS_FOR_CONNECTION_ATTEMPT`)
2. Le vuelve la conexión a 2 y se logra conectar con 3. En este mensaje manda la fecha más reciente del último cambio que tenga. Supongamos `1686509823`
    1. 3 reenvía el mensaje a 0 dado que 2 no está entre él y 0. Se agrega a la lista de por quienes paso el mensaje.
    2. 0 lo reenvía a 1 por los mismos motivos y se agrega a la lista de por quienes paso.
3. 1 recibe el mensaje y ve que 2 está entre 1 y 3. Debe de cambiar su siguiente
    1. Agrega los cuentas cuya actualización más reciente sea mayor a `1686509823`.
    2. Se conecta con 2 pasándole los datos agregados. 2 pisa las cuentas modificadas con estos cambios
    3. 1 cierra su conexión con 3 con un `CloseConnection`
4. La red quedó nuevamente formada


##### Mensaje Token

El mensaje del token es enviado a la red por primera vez por el 0. Este mensaje incluye los siguientes datos.

```rust
type TokenData =  HashMap<usize, Vec<AccountAction>>

struct AccountAction {
    pub message_type: MessageType,
    pub account_id: usize,
    pub points: usize,
    pub last_updated_on: u128,
}
```
El mapa tiene de clave el id del servidor que hizo los cambios y de valor los cambios realizados (las sumas o restas).

![Circulación del token](docs/token-circulando.png)
En la imagen se puede ver como se va pasando el token entre los nodos según el orden.

Los pasos que se ejecutan son los siguientes:
1. Recibo el mensaje de tipo Token desde mi conexión previa `PrevConnection`.
    * Si `PrevConnection` es una nueva conexión establezco el id de quien me envió el token como mi conexión previa.
    * Marco el estado del server como que tiene el token.
    * Limpio el token de mis datos previos y me actualizo con las modificaciones de los otros servidores. *Si algún servidor se perdió en el medio y no limpio sus datos, se evita que se repitan las operaciones con el campo de la fecha de actualización.*
2. Le paso el token a `OrdersManager` por un channel.
    * Este va a ejecutar todas las operaciones que se hayan cargado en `OrdersQueue` hasta que se recibió el token. 
    * Las operaciones de suma (son reducidas si son sobre la misma cuenta)
    * Responde si se pueden hacer las de resta
    * Espera al resultado de los pedidos de resta (**espera por cierto tiempo**, si las cafeteras tardan en responder sale por timeout) y ejecutar la resta
    * Los cambios quedan en la base local y en el token. Se ejecuta 
3. Se envía el token a `NextConnection` por un channel.
    * Si tiene guardadas **sumas de una perdida de conexión con el token** previa las agrega al nuevo token. (Solo guarda las sumas, las restas no se consideran válidas si se perdió la conexión con el token)
    * Envía el mensaje a la siguiente conexión. Si el envío falla, intenta con los siguientes. 
    * Si no logra enviarlo a alguien (crear una nueva conexión) se considera que se perdió la conexión con el token y nos guardamos las sumas.
    * Marcamos que no tenemos el token y se guarda una copia del token si efectivamente se envió.

##### Mensaje Maybe We Lost The Token

#### Modelo


## Dificultades encontradas

## Documentación
La documentación de la aplicación se puede ver con:
```
$ cargo doc --open
```