//! Single-purpose slice extracted from `tui/app.rs` (T5.4 of god-files-2026-07-23-plan.md, ADR D5).

#![allow(dead_code, unused_imports)]

use std::collections::HashMap;
use std::time::Instant;

use theo_domain::event::{DomainEvent, EventType};

use super::*;
use super::state_types::*;

pub fn base64_encode(input: &str) -> String {
    use std::io::Write;
    let mut buf = Vec::new();
    {
        let mut encoder = Base64Encoder::new(&mut buf);
        encoder.write_all(input.as_bytes()).ok();
    }
    String::from_utf8(buf).unwrap_or_default()
}

struct Base64Encoder<'a> {
    out: &'a mut Vec<u8>,
    buf: [u8; 3],
    len: usize,
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

impl<'a> Base64Encoder<'a> {
    fn new(out: &'a mut Vec<u8>) -> Self {
        Self { out, buf: [0; 3], len: 0 }
    }

    fn flush_block(&mut self) {
        let b = &self.buf;
        self.out.push(B64[(b[0] >> 2) as usize]);
        self.out.push(B64[((b[0] & 0x03) << 4 | b[1] >> 4) as usize]);
        if self.len > 1 {
            self.out.push(B64[((b[1] & 0x0f) << 2 | b[2] >> 6) as usize]);
        } else {
            self.out.push(b'=');
        }
        if self.len > 2 {
            self.out.push(B64[(b[2] & 0x3f) as usize]);
        } else {
            self.out.push(b'=');
        }
        self.buf = [0; 3];
        self.len = 0;
    }
}

impl std::io::Write for Base64Encoder<'_> {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        for &byte in data {
            self.buf[self.len] = byte;
            self.len += 1;
            if self.len == 3 {
                self.flush_block();
            }
        }
        Ok(data.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        if self.len > 0 {
            self.flush_block();
        }
        Ok(())
    }
}

impl Drop for Base64Encoder<'_> {
    fn drop(&mut self) {
        let _ = std::io::Write::flush(self);
    }
}

