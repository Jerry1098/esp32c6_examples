use anyhow::{Error, Result};
use defmt::{debug, error, info, Format};
use embassy_executor::Spawner;
use embassy_net::{dns, tcp::TcpSocket, Stack};
use embassy_time::{Duration, Timer};
use rust_mqtt::{
    client::{client::MqttClient, client_config::ClientConfig},
    utils::rng_generator::CountingRng,
};
use smoltcp::wire::DnsQueryType;

pub async fn create_mqtt_client(
    mqtt_username: &'static str,
    mqtt_password: &'static str,
    mqtt_fqdn: &'static str,
    mqtt_port: u16,
    mqtt_client_id: &'static str,
    spawner: Spawner,
    stack: Stack<'static>,
) -> Result<()> {
    let mut rx_buffer = [0; 4096];
    let mut tx_buffer = [0; 4096];

    let mut socket = TcpSocket::new(stack, &mut rx_buffer, &mut tx_buffer);

    debug!("Resolving MQTT FQDN...");
    let mqtt_broker_ip_address = match stack
        .dns_query(mqtt_fqdn, DnsQueryType::A)
        .await
        .map(|a| a[0])
    {
        Ok(address) => address,
        Err(e) => {
            debug!("Error resolving MQTT FQDN: {:?}", e);
            return Err(Error::msg("Failed to resolve MQTT FQDN"));
        }
    };

    let mqtt_endpoint = (mqtt_broker_ip_address, mqtt_port);
    info!("Connecting socket to MQTT broker at {:?}", mqtt_endpoint);

    let connection = socket.connect(mqtt_endpoint).await;

    if let Err(e) = connection {
        debug!("Error connecting socket to MQTT broker: {:?}", e);
        return Err(Error::msg("Failed to connect socket to MQTT broker"));
    }
    info!("Connected socket to MQTT broker!");

    let mut mqtt_config = ClientConfig::new(
        rust_mqtt::client::client_config::MqttVersion::MQTTv5,
        CountingRng(20000),
    );
    mqtt_config.add_username(mqtt_username);
    mqtt_config.add_password(mqtt_password);
    mqtt_config.add_client_id(mqtt_client_id);
    mqtt_config.max_packet_size = 100;

    let mut recv_buffer = [0; 80];
    let mut write_buffer = [0; 80];

    let mut mqtt_client = MqttClient::<_, 5, _>::new(
        socket,
        &mut write_buffer,
        80,
        &mut recv_buffer,
        80,
        mqtt_config,
    );

    info!("Connecting to MQTT broker...");
    match mqtt_client.connect_to_broker().await {
        Ok(()) => {}
        Err(e) => {
            debug!("Error connecting to MQTT broker: {:?}", e);
            return Err(Error::msg("Failed to connect to MQTT broker"));
        }
    }

    mqtt_client.send_ping()
    loop {
        let test_string = "Hello World!";

        match mqtt_client
            .send_message(
                "hello/world",
                test_string.as_bytes(),
                rust_mqtt::packet::v5::publish_packet::QualityOfService::QoS1,
                true,
            )
            .await
        {
            Ok(()) => {
                info!("Message sent successfully!");
            }
            Err(e) => {
                error!("Error sending message: {:?}", e);
            }
        }

        Timer::after(Duration::from_secs(5)).await;
    }
}
