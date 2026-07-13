#![no_main]
use libfuzzer_sys::fuzz_target;
use sniffer_parser::*;
use pnet::packet::ethernet::EthernetPacket;

fuzz_target!(|data: &[u8]| {
    let ethernet_packet = EthernetPacket::new(data);
    match ethernet_packet {
        Some(e_pack) => {
            parse_ethernet_frame(&e_pack, 0);
        },
        _ => ()
    }
    
});