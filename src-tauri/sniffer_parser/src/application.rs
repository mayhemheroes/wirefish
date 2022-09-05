use std::io::Read;
use std::str::from_utf8;
use std::{cell::RefCell, collections::HashMap, net::IpAddr};

use encoding_rs::Encoding;
use flate2::bufread::{DeflateDecoder, GzDecoder, ZlibDecoder};
use httparse::Header;
use mime::Mime;
use tls_parser::{TlsEncryptedContent, parse_tls_record_with_header, TlsRecordType};
use tls_parser::{parse_tls_encrypted, parse_tls_plaintext, parse_tls_raw_record};

use crate::serializable_packet::application::{
    HttpContentType, SerializableHttpRequestPacket, SerializableHttpResponsePacket,
};
use crate::serializable_packet::ParsedPacket;
use crate::SerializablePacket;

#[allow(non_snake_case)]
mod WellKnownPorts {
    pub const HTTP_PORT: u16 = 80;
    pub const TLS_PORT: u16 = 443;
}

// HTTP ----------------------------------------------------------------------------------------------------------------

#[allow(non_snake_case)]
mod ContentEncoding {
    pub const GZIP: &str = "gzip";
    pub const ZLIB: &str = "zlib";
    pub const DEFLATE: &str = "deflate";
}

#[allow(non_snake_case)]
mod HeaderNamesValues {
    pub const CONTENT_ENCODING: &str = "Content-Encoding";
    pub const TRANSFER_ENCODING: &str = "Transfer-Encoding";
    pub const CONTENT_TYPE: &str = "Content-Type";
    pub const CONTENT_LENGTH: &str = "Content-Length";
    pub const CHUNKED: &str = "chunked";
}

pub enum HttpPacketType {
    Request,
    Response,
}

thread_local!(
    static ACTIVE_PARSERS: RefCell<HashMap<((IpAddr,u16),(IpAddr,u16)),Vec<u8>>>
        = RefCell::new(HashMap::new())
);

pub fn handle_application_protocol(
    source_ip: IpAddr,
    source_port: u16,
    dest_ip: IpAddr,
    dest_port: u16,
    is_fin: bool,
    packet: &[u8],
    parsed_packet: &mut ParsedPacket,
) {
    match (source_port, dest_port) {
        (WellKnownPorts::HTTP_PORT, _) | (_, WellKnownPorts::HTTP_PORT) => {
            let http_type = match dest_port {
                WellKnownPorts::HTTP_PORT => HttpPacketType::Request,
                _ => HttpPacketType::Response,
            };

            handle_http_packet(
                source_ip,
                source_port,
                dest_ip,
                dest_port,
                http_type,
                is_fin,
                packet,
                parsed_packet,
            )
        }
        (WellKnownPorts::TLS_PORT, _) | (_, WellKnownPorts::TLS_PORT) => handle_tls_packet(
            source_ip,
            source_port,
            dest_ip,
            dest_port,
            packet,
            parsed_packet,
        ),
        _ => (),
    }
}

pub fn handle_http_packet(
    source_ip: IpAddr,
    source_port: u16,
    dest_ip: IpAddr,
    dest_port: u16,
    http_type: HttpPacketType,
    is_fin: bool,
    packet: &[u8],
    parsed_packet: &mut ParsedPacket,
) {
    ACTIVE_PARSERS.with(|parsers| {
        let mut parsers = parsers.borrow_mut();
        let current_payload = parsers
            .entry(((source_ip, source_port), (dest_ip, dest_port)))
            .and_modify(|payload| payload.append(packet.to_vec().as_mut()))
            .or_insert(packet.to_vec());

        let mut headers = [httparse::EMPTY_HEADER; 1024];

        match http_type {
            HttpPacketType::Request => {
                let mut request = httparse::Request::new(&mut headers);
                let status = request.parse(&current_payload);

                if let Ok(status) = status {
                    if status.is_complete() {
                        let start = status.unwrap();
                        let current_payload_size = current_payload.len() - start;

                        if packet_is_ended(&current_payload[start..], current_payload_size,
                            request.headers, http_type, is_fin)
                        {
                            let parsed_payload = parse_http_payload(
                                current_payload.clone(),
                                start,
                                request.headers,
                            );

                            println!(
                                "[]: HTTP Request Packet: {:?} {:?} {:?}; Headers: {:?}; Payload: {:?}",
                                request.method, request.path, request.version, request.headers, parsed_payload
                            );

                            parsed_packet.set_application_layer_packet(Some(
                                SerializablePacket::HttpRequestPacket(
                                    SerializableHttpRequestPacket::new(&request, parsed_payload),
                                ),
                            ));

                            parsers.remove(&((source_ip, source_port), (dest_ip, dest_port)));

                        }
                    }
                }
            }
            HttpPacketType::Response => {
                let mut response = httparse::Response::new(&mut headers);
                let status = response.parse(current_payload);

                if let Ok(status) = status {
                    if status.is_complete() {
                        let start = status.unwrap();
                        let current_payload_size = current_payload.len() - start;

                        if packet_is_ended(&current_payload[start..], current_payload_size,
                            response.headers, http_type, is_fin)
                        {
                            let parsed_payload = parse_http_payload(
                                current_payload.clone(),
                                start,
                                response.headers,
                            );

                            println!(
                                "[]: HTTP Response Packet: {:?} {:?} {:?}; Headers: {:?}; Payload: {:?}",
                                response.version, response.code, response.reason, response.headers, parsed_payload
                            );

                            parsed_packet.set_application_layer_packet(Some(
                                SerializablePacket::HttpResponsePacket(
                                    SerializableHttpResponsePacket::new(&response, parsed_payload),
                                ),
                            ));

                            parsers.remove(&((source_ip, source_port), (dest_ip, dest_port)));
                        }
                    }
                }
            }
        }
    });
}

// We can say thay an HTTP Request is ended when one the following is true:
// 1. The Request/Response contains the `Content-Length` header and the number of bytes accumulated is the same
// 2. The Request/Response contains the `Transfer-Encoding: chunked` and the last chunk has arrived. THe last chunk
//    it's empty and preceded by a `0` lenght indication.
// 3. The server closes the connection when the Request/Response has been transmitted (FIN-ACK at Transport level)

fn packet_is_ended(
    payload: &[u8],
    current_payload_size: usize,
    headers: &mut [Header],
    http_type: HttpPacketType,
    is_fin_set: bool,
) -> bool {
    let length = get_header_value(HeaderNamesValues::CONTENT_LENGTH, headers);
    let transfer_encoding = get_header_value(HeaderNamesValues::TRANSFER_ENCODING, headers);

    if length.is_none() && transfer_encoding.is_none() {
        return match http_type {
            HttpPacketType::Request => true,
            HttpPacketType::Response => is_fin_set,
        }
    }

    // If Content-Lenght is equal
    if length.is_some() && current_payload_size == length.unwrap().parse::<usize>().unwrap() {
        return true;
    }

    // If Transfer-Encoding is chuncked and last chunck arrived
    if transfer_encoding.is_some() && transfer_encoding.unwrap() == HeaderNamesValues::CHUNKED {
        let last_bytes = payload.into_iter().rev().take(5).collect::<Vec<&u8>>();
        let mut i = 0;

        while i < last_bytes.len() {
            let seq = "\n\r\n\r0".as_bytes().get(i);
            let pay = last_bytes.get(i);

            if seq.is_none() || pay.is_none() || seq.unwrap() != *pay.unwrap() {
                break;
            }

            i += 1;
        }

        if i == last_bytes.len() {
            return true;
        }
    }

    false
}

fn parse_http_payload(
    payload_with_headers: Vec<u8>,
    start: usize,
    headers: &mut [Header],
) -> HttpContentType {
    let mut payload = payload_with_headers[start..].to_vec();
    if payload.is_empty() {
        return HttpContentType::None;
    }

    let transfer_encoding = get_header_value(HeaderNamesValues::TRANSFER_ENCODING, headers);
    if transfer_encoding.is_some() && transfer_encoding.unwrap() == HeaderNamesValues::CHUNKED {
        payload = merge_chunks(payload);
    }

    let mime = get_header_value(HeaderNamesValues::CONTENT_TYPE, headers);
    if mime.is_none() {
        return HttpContentType::Unknown(payload);
    }
    let mime = mime.unwrap().parse::<Mime>();
    if mime.is_err() {
        return HttpContentType::Unknown(payload);
    }

    let mime = mime.unwrap();
    let encoding = get_header_value(HeaderNamesValues::CONTENT_ENCODING, headers);

    return match encoding {
        Some(encoding) => {
            let result = decode_payload(&mut payload, encoding);
            return match result {
                Ok(decoded_payload) => get_http_type(mime, decoded_payload.to_vec(), None),
                Err(algo) => get_http_type(mime, payload.to_vec(), Some(algo)),
            };
        }
        None => get_http_type(mime, payload.to_vec(), None),
    };
}

fn merge_chunks(payload: Vec<u8>) -> Vec<u8> {
    let mut merged = vec![];
    let mut index = 0;
    let mut length = "".to_owned();

    loop {
        length.clear();
        loop {
            if payload[index] == b"\r"[0] && payload[index + 1] == b"\n"[0] {
                break;
            }

            length.push(char::from_u32(payload[index] as u32).unwrap());
            index += 1;
        }

        println!("Length: {}", length);
        let length = usize::from_str_radix(&length, 16).unwrap();

        // Skip \r\n
        index += 2;

        for _ in 0..length {
            merged.push(payload[index]);
            index += 1;
        }

        // Skip \r\n
        index += 2;

        // If last chunk
        if payload[index] == b"0"[0]
            && payload[index + 1] == b"\r"[0]
            && payload[index + 2] == b"\n"[0]
            && payload[index + 3] == b"\r"[0]
            && payload[index + 4] == b"\n"[0]
        {
            break;
        }
    }

    merged
}

fn get_header_value<'a, 'b>(name: &'a str, headers: &'b [Header]) -> Option<&'b str> {
    let header = headers.iter().find(|h| h.name == name);

    match header {
        Some(h) => from_utf8(h.value).ok(),
        None => None,
    }
}

fn get_http_type(mime: Mime, payload: Vec<u8>, encoding: Option<&str>) -> HttpContentType {
    return match (mime.type_(), mime.subtype()) {
        (_, _) if encoding.is_some() => {
            HttpContentType::Encoded(encoding.unwrap().to_string(), payload)
        }
        (mime::TEXT, _) => {
            let charset = mime.get_param(mime::CHARSET);

            if let Some(charset) = charset {
                let encoded = Encoding::for_label(charset.as_str().as_bytes())
                    .unwrap()
                    .decode_with_bom_removal(payload.as_slice());

                return match encoded {
                    (string, false) => HttpContentType::TextCorrectlyDecoded(string.to_string()),
                    (string, true) => HttpContentType::TextMalformedDecoded(string.to_string()),
                };
            }

            HttpContentType::TextDefaultDecoded(String::from_utf8_lossy(&payload).to_string())
        }
        (mime::IMAGE, _) => HttpContentType::Image(payload.to_vec()),
        (mime::MULTIPART, _) => HttpContentType::Multipart(payload.to_vec()),
        _ => HttpContentType::Unknown(payload.to_vec()),
    };
}

fn decode_payload<'a>(payload: &mut Vec<u8>, encoding: &'a str) -> Result<Vec<u8>, &'a str> {
    let mut extensions = encoding.split(", ").collect::<Vec<&str>>();
    extensions.reverse();

    let mut final_decoded = vec![];

    for ext in extensions {
        match ext {
            ContentEncoding::GZIP => {
                let mut decoder = GzDecoder::new(payload.as_slice());
                match decoder.read_to_end(&mut final_decoded) {
                    Err(_) => return Err(ext),
                    _ => (),
                }
            }
            ContentEncoding::ZLIB => {
                let mut decoder = ZlibDecoder::new(payload.as_slice());
                match decoder.read_to_end(&mut final_decoded) {
                    Err(_) => return Err(ext),
                    _ => (),
                }
            }
            ContentEncoding::DEFLATE => {
                let mut decoder = DeflateDecoder::new(payload.as_slice());
                match decoder.read_to_end(&mut final_decoded) {
                    Err(_) => return Err(ext),
                    _ => (),
                }
            }
            _ => return Err(ext),
        }

        payload.clone_from(&final_decoded);
    }

    Ok(final_decoded)
}

// TLS ----------------------------------------------------------------------------------------------------------------

fn handle_tls_packet(
    source_ip: IpAddr,
    source_port: u16,
    dest_ip: IpAddr,
    dest_port: u16,
    packet: &[u8],
    parsed_packet: &mut ParsedPacket,
) {
    ACTIVE_PARSERS.with(|parsers| {
        let mut parsers = parsers.borrow_mut();
        let current_payload = parsers
            .entry(((source_ip, source_port), (dest_ip, dest_port)))
            .and_modify(|payload| payload.append(packet.to_vec().as_mut()))
            .or_insert(packet.to_vec());

        ////////////////////////////
        
        loop {
            let result = parse_tls_raw_record(current_payload);
            match result {
                Ok((rem, record)) => {

                    match record.hdr.record_type {
                        TlsRecordType::ApplicationData => {
                            let result = parse_tls_encrypted(current_payload);
                            match result {
                                Ok((rem, record)) => {
                                    println!(
                                        "[]: TLS Encrypted Packet: {}:{} > {}:{}; Version: {}, Record Type: {:?}, Len: {}",
                                        source_ip, source_port, dest_ip, dest_port, record.hdr.version, record.hdr.record_type, record.hdr.len
                                    );

                                    if rem.is_empty() {
                                        parsers.remove(&((source_ip, source_port), (dest_ip, dest_port)));
                                        break;
                                    } else {
                                        let end = current_payload.len() - rem.len();
                                        current_payload.drain(..end);
                                        continue;
                                    }
                                }
                                Err(tls_parser::nom::Err::Incomplete(needed)) => {
                                    println!("[ERROR] Incomplete TLS: {:?}", needed);
                                    parsers.remove(&((source_ip, source_port), (dest_ip, dest_port)));
                                    break;
                                }
                                Err(tls_parser::nom::Err::Error(e)) => {
                                    println!("[ERROR] Malformed TLS: {:?}", e.code);
                                    parsers.remove(&((source_ip, source_port), (dest_ip, dest_port)));
                                    break;
                                }
                                Err(tls_parser::nom::Err::Failure(_)) => {
                                    println!("[FAILURE] Malformed TLS");
                                    parsers.remove(&((source_ip, source_port), (dest_ip, dest_port)));
                                    break;
                                }
                            }
                        },
                        _ =>  {
                            let result = parse_tls_record_with_header(record.data, &record.hdr);
                            match result {
                                Ok((_, messages)) => {
                                    for (i, msg) in messages.iter().enumerate() {
                                        println!(
                                            "[{i}]: TLS Record Packet: {}:{} > {}:{}; Version: {}, Record Type: {:?}, Len: {}, Payload: {:?}",
                                            source_ip, source_port, dest_ip, dest_port, record.hdr.version, record.hdr.record_type, record.hdr.len, msg
                                        );
                                    }
                                },
                                Err(tls_parser::nom::Err::Incomplete(_)) => {
                                    // Needs defragmentation
                                    break;
                                },
                                Err(tls_parser::nom::Err::Error(e)) => {
                                    println!("[ERROR] Malformed TLS: {:?}", e.code);
                                    parsers.remove(&((source_ip, source_port), (dest_ip, dest_port)));
                                    break;
                                }
                                Err(tls_parser::nom::Err::Failure(_)) => {
                                    println!("[FAILURE] Malformed TLS");
                                    parsers.remove(&((source_ip, source_port), (dest_ip, dest_port)));
                                    break;
                                }
                            };

                            if rem.is_empty() {
                                parsers.remove(&((source_ip, source_port), (dest_ip, dest_port)));
                                break;
                            } else {
                                let end = current_payload.len() - rem.len();
                                current_payload.drain(..end);
                            }
                        }
                    }
                },
                Err(tls_parser::nom::Err::Incomplete(_)) => {
                    break;
                },
                Err(tls_parser::nom::Err::Error(e)) => {
                    println!("[INFO - ERROR] {}:{} > {}:{}; Malformed TLS: {:?}", source_ip, source_port, dest_ip, dest_port, e.code);
                    parsers.remove(&((source_ip, source_port), (dest_ip, dest_port)));
                    break;
                }
                Err(tls_parser::nom::Err::Failure(_)) => {
                    println!("[INFO - FAILURE] Malformed TLS");
                    parsers.remove(&((source_ip, source_port), (dest_ip, dest_port)));
                    break;
                }
            }
        }

       /* while !current_payload.is_empty() {
            let result = parse_tls_plaintext(current_payload);
            match result {
                Ok((rem, record)) => {
                    for (i, msg) in record.msg.iter().enumerate() {
                        println!(
                            "[{i}]: TLS Record Packet: {}:{} > {}:{}; Version: {}, Record Type: {:?}, Len: {}, Payload: {:?}",
                            source_ip, source_port, dest_ip, dest_port, record.hdr.version, record.hdr.record_type, record.hdr.len, msg
                        );
                    }
    
                    if rem.is_empty() {
                        parsers.remove(&((source_ip, source_port), (dest_ip, dest_port)));
                        break;
                    } else {
                        let end = current_payload.len() - rem.len();
                        current_payload.drain(..end);
                        continue;
                    }
                },
                Err(tls_parser::nom::Err::Incomplete(_)) => {
                    break;
                },
                Err(tls_parser::nom::Err::Error(e)) => {
                    match e.code {
                        tls_parser::nom::error::ErrorKind::Many1 => (),
                        _ => {
                            println!("[INFO - ERROR] Malformed TLS: {:?}", e.code);
                            parsers.remove(&((source_ip, source_port), (dest_ip, dest_port)));
                            break;
                        }
                    }
                }
                Err(tls_parser::nom::Err::Failure(_)) => {
                    println!("[INFO - FAILURE] Malformed TLS");
                    parsers.remove(&((source_ip, source_port), (dest_ip, dest_port)));
                    break;
                }
            }

            let result = parse_tls_encrypted(current_payload);
            match result {
                Ok((rem, record)) => {
                    println!(
                        "[]: TLS Encrypted Packet: {}:{} > {}:{}; Version: {}, Record Type: {:?}, Len: {}",
                        source_ip, source_port, dest_ip, dest_port, record.hdr.version, record.hdr.record_type, record.hdr.len
                    );

                    if rem.is_empty() {
                        parsers.remove(&((source_ip, source_port), (dest_ip, dest_port)));
                        break;
                    } else {
                        let end = current_payload.len() - rem.len();
                        current_payload.drain(..end);
                        continue;
                    }
                }
                Err(tls_parser::nom::Err::Incomplete(needed)) => {
                    println!("[ERROR] Incomplete TLS: {:?}", needed);
                    parsers.remove(&((source_ip, source_port), (dest_ip, dest_port)));
                    break;
                }
                Err(tls_parser::nom::Err::Error(e)) => {
                    println!("[ERROR] Malformed TLS: {:?}", e.code);
                    parsers.remove(&((source_ip, source_port), (dest_ip, dest_port)));
                    break;
                }
                Err(tls_parser::nom::Err::Failure(_)) => {
                    println!("[FAILURE] Malformed TLS");
                    parsers.remove(&((source_ip, source_port), (dest_ip, dest_port)));
                    break;
                }
            }            
        } */
    });
}
