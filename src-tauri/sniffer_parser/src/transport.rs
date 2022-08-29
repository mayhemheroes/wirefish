use pnet::packet::icmp::{echo_reply, echo_request, IcmpPacket, IcmpTypes};
use pnet::packet::icmpv6::Icmpv6Packet;
use pnet::packet::ip::{IpNextHeaderProtocol, IpNextHeaderProtocols};
use pnet::packet::tcp::TcpPacket;
use pnet::packet::udp::UdpPacket;

use std::net::IpAddr;

use crate::serializable_packet::transport::{
    SerializableEchoReplyPacket, SerializableEchoRequestPacket, SerializableIcmpPacket,
    SerializableIcmpv6Packet, SerializableTcpPacket, SerializableUdpPacket,
};

use super::*;

pub fn handle_udp_packet(
    source: IpAddr,
    destination: IpAddr,
    packet: &[u8],
    parsed_packet: &mut ParsedPacket,
) {
    let udp = UdpPacket::new(packet);

    if let Some(udp) = udp {
        println!(
            "[]: UDP Packet: {}:{} > {}:{}; length: {}",
            source,
            udp.get_source(),
            destination,
            udp.get_destination(),
            udp.get_length()
        );

        parsed_packet.set_transport_layer_packet(Some(SerializablePacket::UdpPacket(
            SerializableUdpPacket::from(&udp),
        )));
    } else {
        println!("[]: Malformed UDP Packet");
        parsed_packet.set_transport_layer_packet(Some(SerializablePacket::MalformedPacket(
            "Malformed UDP Packet".to_string(),
        )));
    }
}

pub fn handle_tcp_packet(
    source: IpAddr,
    destination: IpAddr,
    packet: &[u8],
    parsed_packet: &mut ParsedPacket,
) {
    let tcp = TcpPacket::new(packet);
    if let Some(tcp) = tcp {
        println!(
            "[]: TCP Packet: {}:{} > {}:{}; length: {}",
            source,
            tcp.get_source(),
            destination,
            tcp.get_destination(),
            packet.len()
        );

        parsed_packet.set_transport_layer_packet(Some(SerializablePacket::TcpPacket(
            SerializableTcpPacket::from(&tcp),
        )));
    } else {
        println!("[]: Malformed TCP Packet");
        parsed_packet.set_transport_layer_packet(Some(SerializablePacket::MalformedPacket(
            "Malformed TCP Packet".to_string(),
        )));
    }
}

pub fn handle_transport_protocol(
    source: IpAddr,
    destination: IpAddr,
    protocol: IpNextHeaderProtocol,
    packet: &[u8],
    parsed_packet: &mut ParsedPacket,
) {
    return match protocol {
        IpNextHeaderProtocols::Udp => handle_udp_packet(source, destination, packet, parsed_packet),
        IpNextHeaderProtocols::Tcp => handle_tcp_packet(source, destination, packet, parsed_packet),
        IpNextHeaderProtocols::Icmp => {
            handle_icmp_packet(source, destination, packet, parsed_packet)
        }
        IpNextHeaderProtocols::Icmpv6 => {
            handle_icmpv6_packet(source, destination, packet, parsed_packet)
        }
        _ => {
            println!(
                "[]: Unknown {} packet: {} > {}; protocol: {:?} length: {}",
                match source {
                    IpAddr::V4(..) => "IPv4",
                    _ => "IPv6",
                },
                source,
                destination,
                protocol,
                packet.len()
            );
        }
    };
}

pub fn handle_icmp_packet(
    source: IpAddr,
    destination: IpAddr,
    packet: &[u8],
    parsed_packet: &mut ParsedPacket,
) {
    let icmp_packet = IcmpPacket::new(packet);
    if let Some(icmp_packet) = icmp_packet {
        match icmp_packet.get_icmp_type() {
            IcmpTypes::EchoReply => {
                let echo_reply_packet = echo_reply::EchoReplyPacket::new(packet).unwrap();
                println!(
                    "[]: ICMP echo reply {} -> {} (seq={:?}, id={:?})",
                    source,
                    destination,
                    echo_reply_packet.get_sequence_number(),
                    echo_reply_packet.get_identifier(),
                );

                parsed_packet.set_transport_layer_packet(Some(
                    SerializablePacket::EchoReplyPacket(SerializableEchoReplyPacket::from(
                        &echo_reply_packet,
                    )),
                ));
            }
            IcmpTypes::EchoRequest => {
                let echo_request_packet = echo_request::EchoRequestPacket::new(packet).unwrap();
                println!(
                    "[]: ICMP echo request {} -> {} (seq={:?}, id={:?})",
                    source,
                    destination,
                    echo_request_packet.get_sequence_number(),
                    echo_request_packet.get_identifier()
                );

                parsed_packet.set_transport_layer_packet(Some(
                    SerializablePacket::EchoRequestPacket(SerializableEchoRequestPacket::from(
                        &echo_request_packet,
                    )),
                ));
            }
            _ => {
                println!(
                    "[]: ICMP packet {} -> {} (code={:?}, type={:?})",
                    source,
                    destination,
                    icmp_packet.get_icmp_code(),
                    icmp_packet.get_icmp_type()
                );

                parsed_packet.set_transport_layer_packet(Some(SerializablePacket::IcmpPacket(
                    SerializableIcmpPacket::from(&icmp_packet),
                )));
            }
        }
    } else {
        println!("[]: Malformed ICMP Packet");
        parsed_packet.set_transport_layer_packet(Some(SerializablePacket::MalformedPacket(
            "Malformed ICMP Packet".to_string(),
        )));
    }
}

pub fn handle_icmpv6_packet(
    source: IpAddr,
    destination: IpAddr,
    packet: &[u8],
    parsed_packet: &mut ParsedPacket,
) {
    let icmpv6_packet = Icmpv6Packet::new(packet);
    if let Some(icmpv6_packet) = icmpv6_packet {
        println!(
            "[]: ICMPv6 packet {} -> {} (type={:?})",
            source,
            destination,
            icmpv6_packet.get_icmpv6_type()
        );

        parsed_packet.set_transport_layer_packet(Some(SerializablePacket::Icmpv6Packet(
            SerializableIcmpv6Packet::from(&icmpv6_packet),
        )));
    } else {
        println!("[]: Malformed ICMPv6 Packet");
        parsed_packet.set_transport_layer_packet(Some(SerializablePacket::MalformedPacket(
            "Malformed ICMPv6 Packet".to_string(),
        )));
    }
}

#[cfg(test)]
mod tests {
    use std::net::IpAddr;
    use std::net::Ipv4Addr;

    use pnet::packet::icmp::IcmpPacket;
    use pnet::packet::icmpv6::echo_reply::Icmpv6Codes;
    use pnet::packet::icmpv6::Icmpv6Types;
    use pnet::packet::icmpv6::MutableIcmpv6Packet;
    use pnet::packet::tcp::MutableTcpPacket;
    use pnet::packet::tcp::TcpPacket;
    use pnet::packet::udp::MutableUdpPacket;
    use pnet::packet::udp::UdpPacket;
    use pnet::packet::Packet;

    use super::*;

    #[test]
    fn valid_udp_packet() {
        let mut udp_buffer = [0u8; 42];

        let udp_packet = build_test_udp_packet(udp_buffer.as_mut_slice());
        let mut parsed_packet = ParsedPacket::new();
        handle_udp_packet(
            IpAddr::V4(Ipv4Addr::new(10, 10, 10, 10)),
            IpAddr::V4(Ipv4Addr::new(11, 11, 11, 11)),
            udp_packet.packet(),
            &mut parsed_packet,
        );

        if let SerializablePacket::UdpPacket(new_udp_packet) =
            parsed_packet.get_transport_layer_packet().unwrap()
        {
            assert_eq!(new_udp_packet.source, udp_packet.get_source());
            assert_eq!(new_udp_packet.destination, udp_packet.get_destination());
            assert_eq!(new_udp_packet.length, udp_packet.get_length());
            assert_eq!(new_udp_packet.checksum, udp_packet.get_checksum());
            assert_eq!(new_udp_packet.payload, udp_packet.payload().to_vec());
        }
    }

    #[test]
    fn valid_tcp_packet() {
        let mut tcp_buffer = [0u8; 42];

        let tcp_packet = build_test_tcp_packet(tcp_buffer.as_mut_slice());
        let mut parsed_packet = ParsedPacket::new();
        handle_tcp_packet(
            IpAddr::V4(Ipv4Addr::new(10, 10, 10, 10)),
            IpAddr::V4(Ipv4Addr::new(11, 11, 11, 11)),
            tcp_packet.packet(),
            &mut parsed_packet,
        );

        if let SerializablePacket::TcpPacket(new_tcp_packet) =
            parsed_packet.get_transport_layer_packet().unwrap()
        {
            assert_eq!(new_tcp_packet.source, tcp_packet.get_source());
            assert_eq!(new_tcp_packet.destination, tcp_packet.get_destination());
            assert_eq!(new_tcp_packet.sequence, tcp_packet.get_sequence());
            assert_eq!(
                new_tcp_packet.acknowledgement,
                tcp_packet.get_acknowledgement()
            );
            assert_eq!(new_tcp_packet.data_offset, tcp_packet.get_data_offset());
            assert_eq!(new_tcp_packet.reserved, tcp_packet.get_reserved());
            assert_eq!(new_tcp_packet.flags, tcp_packet.get_flags());
            assert_eq!(new_tcp_packet.window, tcp_packet.get_window());
            assert_eq!(new_tcp_packet.checksum, tcp_packet.get_checksum());
            assert_eq!(new_tcp_packet.urgent_ptr, tcp_packet.get_urgent_ptr());
            assert_eq!(new_tcp_packet.options, tcp_packet.get_options_raw());
            assert_eq!(new_tcp_packet.payload, tcp_packet.payload().to_vec());
        }
    }

    #[test]
    fn valid_icmp_echo_reply_packet() {
        let mut icmp_buffer = [0u8; 42];

        let echo_reply_packet = echo_reply::EchoReplyPacket::new(&mut icmp_buffer).unwrap();
        let mut parsed_packet = ParsedPacket::new();
        handle_icmp_packet(
            IpAddr::V4(Ipv4Addr::new(10, 10, 10, 10)),
            IpAddr::V4(Ipv4Addr::new(11, 11, 11, 11)),
            echo_reply_packet.packet(),
            &mut parsed_packet,
        );

        if let SerializablePacket::EchoReplyPacket(new_echo_reply_packet) =
            parsed_packet.get_transport_layer_packet().unwrap()
        {
            assert_eq!(
                new_echo_reply_packet.icmp_type,
                echo_reply_packet.get_icmp_type().0
            );
            assert_eq!(
                new_echo_reply_packet.icmp_code,
                echo_reply_packet.get_icmp_code().0
            );
            assert_eq!(
                new_echo_reply_packet.checksum,
                echo_reply_packet.get_checksum()
            );
            assert_eq!(
                new_echo_reply_packet.identifier,
                echo_reply_packet.get_identifier()
            );
            assert_eq!(
                new_echo_reply_packet.sequence_number,
                echo_reply_packet.get_sequence_number()
            );
            assert_eq!(
                new_echo_reply_packet.payload,
                echo_reply_packet.payload().to_vec()
            );
        }
    }

    #[test]
    fn valid_icmp_echo_request_packet() {
        let mut icmp_buffer = [0u8; 42];
        let mut echo_request_packet =
            echo_request::MutableEchoRequestPacket::new(&mut icmp_buffer).unwrap();

        echo_request_packet.set_icmp_type(IcmpTypes::EchoRequest);

        let mut parsed_packet = ParsedPacket::new();
        handle_icmp_packet(
            IpAddr::V4(Ipv4Addr::new(10, 10, 10, 10)),
            IpAddr::V4(Ipv4Addr::new(11, 11, 11, 11)),
            echo_request_packet.packet(),
            &mut parsed_packet,
        );

        if let SerializablePacket::EchoRequestPacket(new_echo_reply_packet) =
            parsed_packet.get_transport_layer_packet().unwrap()
        {
            assert_eq!(
                new_echo_reply_packet.icmp_type,
                echo_request_packet.get_icmp_type().0
            );
            assert_eq!(
                new_echo_reply_packet.icmp_code,
                echo_request_packet.get_icmp_code().0
            );
            assert_eq!(
                new_echo_reply_packet.checksum,
                echo_request_packet.get_checksum()
            );
            assert_eq!(
                new_echo_reply_packet.identifier,
                echo_request_packet.get_identifier()
            );
            assert_eq!(
                new_echo_reply_packet.sequence_number,
                echo_request_packet.get_sequence_number()
            );
            assert_eq!(
                new_echo_reply_packet.payload,
                echo_request_packet.payload().to_vec()
            );
        }
    }

    #[test]
    fn unrecognized_icmp_packet() {
        let mut icmp_buffer = [0u8; 42];
        let icmp_packet = IcmpPacket::new(&mut icmp_buffer).unwrap();

        let mut parsed_packet = ParsedPacket::new();
        handle_icmp_packet(
            IpAddr::V4(Ipv4Addr::new(10, 10, 10, 10)),
            IpAddr::V4(Ipv4Addr::new(11, 11, 11, 11)),
            icmp_packet.packet(),
            &mut parsed_packet,
        );

        if let SerializablePacket::IcmpPacket(new_icmp_packet) =
            parsed_packet.get_transport_layer_packet().unwrap()
        {
            assert_eq!(new_icmp_packet.icmp_type, icmp_packet.get_icmp_type().0);
            assert_eq!(new_icmp_packet.icmp_code, icmp_packet.get_icmp_code().0);
            assert_eq!(new_icmp_packet.checksum, icmp_packet.get_checksum());
            assert_eq!(new_icmp_packet.payload, icmp_packet.payload().to_vec());
        }
    }

    #[test]
    fn valid_icmpv6_packet() {
        let mut icmpv6_buffer = [0u8; 42];

        let icmpv6_packet = build_test_icmpv6_packet(&mut icmpv6_buffer);
        let mut parsed_packet = ParsedPacket::new();
        handle_icmpv6_packet(
            IpAddr::V4(Ipv4Addr::new(10, 10, 10, 10)),
            IpAddr::V4(Ipv4Addr::new(11, 11, 11, 11)),
            icmpv6_packet.packet(),
            &mut parsed_packet,
        );

        if let SerializablePacket::Icmpv6Packet(new_icmpv6_packet) =
            parsed_packet.get_transport_layer_packet().unwrap()
        {
            assert_eq!(
                new_icmpv6_packet.icmpv6_type,
                icmpv6_packet.get_icmpv6_type().0
            );
            assert_eq!(
                new_icmpv6_packet.icmpv6_code,
                icmpv6_packet.get_icmpv6_code().0
            );
            assert_eq!(new_icmpv6_packet.checksum, icmpv6_packet.get_checksum());
            assert_eq!(new_icmpv6_packet.payload, icmpv6_packet.payload().to_vec());
        }
    }

    ///////////////////// Utils

    fn build_test_udp_packet<'a>(udp_buffer: &'a mut [u8]) -> UdpPacket<'a> {
        let mut udp_packet = MutableUdpPacket::new(udp_buffer).unwrap();

        udp_packet.set_source(4444);
        udp_packet.set_destination(4445);

        udp_packet.consume_to_immutable()
    }

    fn build_test_tcp_packet<'a>(tcp_buffer: &'a mut [u8]) -> TcpPacket<'a> {
        let mut tcp_packet = MutableTcpPacket::new(tcp_buffer).unwrap();

        tcp_packet.set_source(4444);
        tcp_packet.set_destination(4445);

        tcp_packet.consume_to_immutable()
    }

    fn build_test_icmpv6_packet<'a>(icmpv6_buffer: &'a mut [u8]) -> Icmpv6Packet<'a> {
        let mut icmpv6_packet = MutableIcmpv6Packet::new(icmpv6_buffer).unwrap();

        icmpv6_packet.set_icmpv6_code(Icmpv6Codes::NoCode);
        icmpv6_packet.set_icmpv6_type(Icmpv6Types::EchoReply);

        icmpv6_packet.consume_to_immutable()
    }
}