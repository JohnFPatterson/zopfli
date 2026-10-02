//! gzip, zlib, and raw DEFLATE containers.
//!
//! Checksums use the tables and reduction order from the C containers.
//! CRC32 is accumulated in `u32`. Adler32 starts from `s1 = 1` and `s2 = 1 >> 16`
//! (which is 0), reduces modulo 65521 every 5550 bytes, and checksums only the
//! first `(len as u32)` bytes.

use crate::deflate::deflate_into;
use crate::Options;

/// CRC-32 table, polynomial `0xedb88320`.
const CRC32_TABLE: [u32; 256] = [
    0, 1996959894, 3993919788, 2567524794, 124634137, 1886057615, 3915621685, 2657392035,
    249268274, 2044508324, 3772115230, 2547177864, 162941995, 2125561021, 3887607047, 2428444049,
    498536548, 1789927666, 4089016648, 2227061214, 450548861, 1843258603, 4107580753, 2211677639,
    325883990, 1684777152, 4251122042, 2321926636, 335633487, 1661365465, 4195302755, 2366115317,
    997073096, 1281953886, 3579855332, 2724688242, 1006888145, 1258607687, 3524101629, 2768942443,
    901097722, 1119000684, 3686517206, 2898065728, 853044451, 1172266101, 3705015759, 2882616665,
    651767980, 1373503546, 3369554304, 3218104598, 565507253, 1454621731, 3485111705, 3099436303,
    671266974, 1594198024, 3322730930, 2970347812, 795835527, 1483230225, 3244367275, 3060149565,
    1994146192, 31158534, 2563907772, 4023717930, 1907459465, 112637215, 2680153253, 3904427059,
    2013776290, 251722036, 2517215374, 3775830040, 2137656763, 141376813, 2439277719, 3865271297,
    1802195444, 476864866, 2238001368, 4066508878, 1812370925, 453092731, 2181625025, 4111451223,
    1706088902, 314042704, 2344532202, 4240017532, 1658658271, 366619977, 2362670323, 4224994405,
    1303535960, 984961486, 2747007092, 3569037538, 1256170817, 1037604311, 2765210733, 3554079995,
    1131014506, 879679996, 2909243462, 3663771856, 1141124467, 855842277, 2852801631, 3708648649,
    1342533948, 654459306, 3188396048, 3373015174, 1466479909, 544179635, 3110523913, 3462522015,
    1591671054, 702138776, 2966460450, 3352799412, 1504918807, 783551873, 3082640443, 3233442989,
    3988292384, 2596254646, 62317068, 1957810842, 3939845945, 2647816111, 81470997, 1943803523,
    3814918930, 2489596804, 225274430, 2053790376, 3826175755, 2466906013, 167816743, 2097651377,
    4027552580, 2265490386, 503444072, 1762050814, 4150417245, 2154129355, 426522225, 1852507879,
    4275313526, 2312317920, 282753626, 1742555852, 4189708143, 2394877945, 397917763, 1622183637,
    3604390888, 2714866558, 953729732, 1340076626, 3518719985, 2797360999, 1068828381, 1219638859,
    3624741850, 2936675148, 906185462, 1090812512, 3747672003, 2825379669, 829329135, 1181335161,
    3412177804, 3160834842, 628085408, 1382605366, 3423369109, 3138078467, 570562233, 1426400815,
    3317316542, 2998733608, 733239954, 1555261956, 3268935591, 3050360625, 752459403, 1541320221,
    2607071920, 3965973030, 1969922972, 40735498, 2617837225, 3943577151, 1913087877, 83908371,
    2512341634, 3803740692, 2075208622, 213261112, 2463272603, 3855990285, 2094854071, 198958881,
    2262029012, 4057260610, 1759359992, 534414190, 2176718541, 4139329115, 1873836001, 414664567,
    2282248934, 4279200368, 1711684554, 285281116, 2405801727, 4167216745, 1634467795, 376229701,
    2685067896, 3608007406, 1308918612, 956543938, 2808555105, 3495958263, 1231636301, 1047427035,
    2932959818, 3654703836, 1088359270, 936918000, 2847714899, 3736837829, 1202900863, 817233897,
    3183342108, 3401237130, 1404277552, 615818150, 3134207493, 3453421203, 1423857449, 601450431,
    3009837614, 3294710456, 1567103746, 711928724, 3020668471, 3272380065, 1510334235, 755167117,
];

fn crc32(data: &[u8]) -> u32 {
    let mut result: u32 = 0xffff_ffff;
    for &byte in data {
        let index = ((result ^ u32::from(byte)) & 0xff) as usize;
        result = CRC32_TABLE[index] ^ (result >> 8);
    }
    result ^ 0xffff_ffff
}

fn push_le_u32(out: &mut Vec<u8>, value: u32) {
    out.push((value % 256) as u8);
    out.push(((value >> 8) % 256) as u8);
    out.push(((value >> 16) % 256) as u8);
    out.push(((value >> 24) % 256) as u8);
}

fn push_isize(out: &mut Vec<u8>, len: usize) {
    out.push((len % 256) as u8);
    out.push(((len >> 8) % 256) as u8);
    out.push(((len >> 16) % 256) as u8);
    out.push(((len >> 24) % 256) as u8);
}

/// Adler32 as implemented by `zlib_container.c`.
fn adler32(data: &[u8]) -> u32 {
    const SUMS_OVERFLOW: usize = 5550;
    let mut s1: u32 = 1;
    // C initializes this as `1 >> 16`, which is 0.
    let mut s2: u32 = 1 >> 16;
    let mut offset = 0usize;
    let mut size = data.len();
    while size > 0 {
        let amount = if size > SUMS_OVERFLOW {
            SUMS_OVERFLOW
        } else {
            size
        };
        size -= amount;
        let end = offset + amount;
        while offset < end {
            s1 = s1.wrapping_add(u32::from(data[offset]));
            offset += 1;
            s2 = s2.wrapping_add(s1);
        }
        s1 %= 65521;
        s2 %= 65521;
    }
    (s2 << 16) | s1
}

/// Compress `input` into a gzip member (RFC 1952) and return the bytes.
pub fn gzip_compress(options: &Options, input: &[u8]) -> Vec<u8> {
    let crcvalue = crc32(input);
    let mut out = Vec::new();
    out.extend_from_slice(&[
        31, 139, // ID1, ID2
        8,   // CM
        0,   // FLG
        0, 0, 0, 0, // MTIME
        2, // XFL
        3, // OS
    ]);
    let mut bp = 0u8;
    deflate_into(options, 2, true, input, &mut bp, &mut out);
    push_le_u32(&mut out, crcvalue);
    push_isize(&mut out, input.len());
    out
}

/// Compress `input` into a zlib wrapper (RFC 1950) and return the bytes.
pub fn zlib_compress(options: &Options, input: &[u8]) -> Vec<u8> {
    let checksum_len = (input.len() as u32) as usize;
    let checksum = adler32(&input[..checksum_len]);
    let cmf: u32 = 120;
    let flevel: u32 = 3;
    let fdict: u32 = 0;
    let mut cmfflg = 256 * cmf + fdict * 32 + flevel * 64;
    let fcheck = 31 - cmfflg % 31;
    cmfflg += fcheck;

    let mut out = Vec::new();
    out.push((cmfflg / 256) as u8);
    out.push((cmfflg % 256) as u8);
    let mut bp = 0u8;
    deflate_into(options, 2, true, input, &mut bp, &mut out);
    out.push(((checksum >> 24) % 256) as u8);
    out.push(((checksum >> 16) % 256) as u8);
    out.push(((checksum >> 8) % 256) as u8);
    out.push((checksum % 256) as u8);
    out
}

/// Compress `input` as a raw DEFLATE stream.
///
/// This is `ZopfliCompress` for `ZOPFLI_FORMAT_DEFLATE`: dynamic blocks, final
/// bit set, starting from an empty buffer and bit pointer 0.
pub fn deflate_compress(options: &Options, input: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut bp = 0u8;
    deflate_into(options, 2, true, input, &mut bp, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_matches_iso_and_c_table() {
        assert_eq!(crc32(b""), 0);
        assert_eq!(crc32(b"a"), 0xe8b7_be43);
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
        assert_eq!(crc32(b"hello"), 0x3610_a686);
        assert_eq!(crc32(&vec![0xff; 6000]), 0xb782_9fe5);
    }

    #[test]
    fn adler32_matches_zlib_reduction() {
        assert_eq!(adler32(b""), 1);
        assert_eq!(adler32(b"a"), 0x0062_0062);
        assert_eq!(adler32(b"123456789"), 0x091e_01de);
        assert_eq!(adler32(b"hello"), 0x062c_0215);
        assert_eq!(adler32(&vec![0xff; 6000]), 0xa497_59ea);
        assert_eq!(adler32(&vec![1u8; 20000]), 0xea04_4e21);
    }

    #[test]
    fn empty_containers_match_c() {
        let options = Options::default();
        assert_eq!(deflate_compress(&options, b""), vec![0x03, 0x00]);
        assert_eq!(
            gzip_compress(&options, b""),
            vec![
                0x1f, 0x8b, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0x03, 0x03, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            ]
        );
        assert_eq!(
            zlib_compress(&options, b""),
            vec![0x78, 0xda, 0x03, 0x00, 0x00, 0x00, 0x00, 0x01]
        );
    }

    #[test]
    fn gzip_trailer_uses_crc_and_isize() {
        let options = Options::default();
        let gzip = gzip_compress(&options, b"hello");
        assert_eq!(
            gzip,
            vec![
                0x1f, 0x8b, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0x03, 0xcb, 0x48, 0xcd, 0xc9,
                0xc9, 0x07, 0x00, 0x86, 0xa6, 0x10, 0x36, 0x05, 0x00, 0x00, 0x00,
            ]
        );
    }

    #[test]
    fn zlib_header_is_cmf_flg_with_fcheck() {
        let options = Options::default();
        let zlib = zlib_compress(&options, b"hello");
        assert_eq!(zlib[0], 120);
        assert_eq!(zlib[1], 218);
        let cmfflg = u32::from(zlib[0]) * 256 + u32::from(zlib[1]);
        assert_eq!(cmfflg % 31, 0);
        // Adler32 of "hello" is 0x062c0215, big-endian.
        assert_eq!(&zlib[zlib.len() - 4..], &[0x06, 0x2c, 0x02, 0x15]);
        assert_eq!(
            zlib,
            vec![0x78, 0xda, 0xcb, 0x48, 0xcd, 0xc9, 0xc9, 0x07, 0x00, 0x06, 0x2c, 0x02, 0x15,]
        );
    }
}
