//! An OFX file names its bank (`<ORG>` in the sign-on block), so `fetch plan` can credit the service.

use cashcast_engine::export::LoadOptions;
use cashcast_engine::ofx;

const BODY: &str = "<BANKMSGSRSV1><STMTTRNRS><STMTRS><CURDEF>CAD\n\
    <BANKACCTFROM><BANKID>9<ACCTID>000111222333<ACCTTYPE>CHECKING</BANKACCTFROM>\n\
    <BANKTRANLIST>\n<STMTTRN><TRNTYPE>DEBIT<DTPOSTED>20260905<TRNAMT>-42.10<FITID>A1<NAME>GROCER CANARY31</STMTTRN>\n\
    </BANKTRANLIST></STMTRS></STMTTRNRS></BANKMSGSRSV1>\n</OFX>\n";

fn notes(text: &str) -> Vec<String> {
    ofx::parse(text, "x.qfx", &LoadOptions::default())
        .unwrap()
        .notes
}

#[test]
fn sgml_sign_on_org_becomes_an_institution_note() {
    let text = format!(
        "OFXHEADER:100\nDATA:OFXSGML\n\n<OFX>\n<SIGNONMSGSRSV1><SONRS><STATUS><CODE>0</STATUS>\
         <FI>\n<ORG>CIBC\n<FID>00001\n</FI>\n<INTU.BID>00001\n</SONRS></SIGNONMSGSRSV1>\n{BODY}"
    );
    assert!(notes(&text).contains(&"institution: CIBC".to_string()));
}

#[test]
fn xml_org_with_closing_tag_is_read() {
    let text = format!(
        "<?xml version=\"1.0\"?><OFX><SIGNONMSGSRSV1><SONRS><FI><ORG>TD</ORG><FID>1</FID></FI></SONRS></SIGNONMSGSRSV1>\n{BODY}"
    );
    assert!(notes(&text).contains(&"institution: TD".to_string()));
}

#[test]
fn no_org_means_no_institution_note() {
    let text = format!("OFXHEADER:100\n<OFX>\n{BODY}");
    assert!(!notes(&text).iter().any(|n| n.starts_with("institution")));
}
