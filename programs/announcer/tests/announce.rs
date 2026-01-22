// Host test: the program's own wire constants (instruction discriminator,
// account discriminator, account size) must equal the values in the generated
// IDL. The SDK client (sdk/contract.test.mjs) is asserted against those same IDL
// values, so client and program are pinned to one wire format from both ends.
// Pure anchor-lang, runs on the host with `cargo test -p announcer`.
use anchor_lang::{Discriminator, InstructionData};

#[test]
fn wire_constants_match_idl() {
    // instructions[0].discriminator in announcer.json
    let data = announcer::instruction::Announce {
        r: [0u8; 32],
        view_tag: 0,
        scheme: 0,
    }
    .data();
    assert_eq!(
        &data[..8],
        &[7, 30, 100, 250, 110, 253, 3, 149],
        "announce discriminator"
    );
    // borsh body after the 8-byte discriminator: r[32] + view_tag[1] + scheme[1]
    assert_eq!(
        data.len(),
        8 + 32 + 1 + 1,
        "announce instruction data length"
    );

    // accounts[0].discriminator in announcer.json
    assert_eq!(
        announcer::Announcement::DISCRIMINATOR,
        [73, 38, 210, 135, 9, 143, 191, 105],
        "Announcement account discriminator",
    );

    // 8 discriminator + r[32] + view_tag[1] + scheme[1] + slot[8] = 50
    assert_eq!(
        announcer::Announcement::SPACE,
        8 + 32 + 1 + 1 + 8,
        "Announcement account size"
    );
}
