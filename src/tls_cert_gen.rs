use std::fs::File;
use std::io::Write;
use log::{debug, error};
use openssl::asn1::Asn1Time;
use openssl::bn::{BigNum, MsbOption};
use openssl::error::ErrorStack;
use openssl::hash::MessageDigest;
use openssl::pkey::{PKey, PKeyRef, Private};
use openssl::rsa::Rsa;
use openssl::x509::extension::{AuthorityKeyIdentifier, BasicConstraints, KeyUsage, SubjectAlternativeName, SubjectKeyIdentifier};
use openssl::x509::{X509Name, X509NameBuilder, X509Ref, X509Req, X509ReqBuilder, X509VerifyResult, X509};

// need to preinstall openssl on machine before able to run this

// you can either let rust do it for you by enabling the "vendored" feature on the openssl crate like below
// openssl = { version = "0.10.70", features = ["vendored"] }
// do note u need to install perl to proceed this way, i got my copy here: https://strawberryperl.com/
// also if u do it this way be ready for the compiling process to take literal minutes
// mine took 8 ( ͡° ͜ʖ ͡°)

// or install it yourself beforehand
// i recommend do it with vcpkg: https://learn.microsoft.com/en-gb/vcpkg/get_started/get-started?pivots=shell-powershell
// and you need to set up so many environment vars, google them for the full list

// most of the following code is from https://github.com/sfackler/rust-openssl/blob/master/openssl/examples/mk_certs.rs

// Make a Certificate Authority certificate and private key
fn mk_ca_cert() -> Result<(X509, PKey<Private>), ErrorStack> {
    // Generate a private key
    let private_key = PKey::from_rsa(Rsa::generate(2048)?)?;

    // fill in the metadata
    let mut x509name = X509Name::builder()?;
    x509name.append_entry_by_text("C", "HK")?; // country
    // x509name.append_entry_by_text("ST", "")?; // state
    x509name.append_entry_by_text("L", "Chai Wan")?; // locale(city)
    x509name.append_entry_by_text("O", "The Four-word mantra organization")?; // organization
    x509name.append_entry_by_text("CN", "DistinctlyLookingLovableMothers.com")?; // the (theoretic) domain of the authority
    let x509name = x509name.build(); // pack the metadata

    // make the certificate
    let mut cert = X509::builder()?;
    cert.set_version(3)?; // set cert version
    let serial_number = { // make up the serial number which is just a number to identify the cert
        let mut serial = BigNum::new()?;
        serial.rand(159, MsbOption::MAYBE_ZERO, false)?;
        serial.to_asn1_integer()?
    };
    cert.set_serial_number(&serial_number)?; // set the serial number
    cert.set_subject_name(&x509name)?; // set the metadata (defined above)
    cert.set_issuer_name(&x509name)?; // set the metadata (defined above)
    cert.set_pubkey(&private_key)?; // generate the public key with the previously made private key
    cert.set_not_before(Asn1Time::days_from_now(0)?.as_ref())?; // set the start date
    cert.set_not_after(Asn1Time::days_from_now(365)?.as_ref())?; // set the end date

    // flag this cert as from an authority, critical() flags this extension as a must have
    cert.append_extension(BasicConstraints::new().critical().ca().build()?)?;

    // prevents the key to be backupped and be used for any other purpose, in case someone got the key when theyre not supposed to
    cert.append_extension(KeyUsage::new().critical().key_cert_sign().crl_sign().build()?)?;

    // provides an identifier to the public key similar to fingerprint except baked into the cert itself
    cert.append_extension(SubjectKeyIdentifier::new().build(&cert.x509v3_context(None, None))?)?;

    cert.sign(&private_key, MessageDigest::sha256())?; // sign the cert with the private key and hash it in sha256
    let cert = cert.build(); // package the cert

    // Save the private key and certificate in PEM format
    // let mut privkey_file = File::create("localhost.key").unwrap();
    // let mut cert_file = File::create("localhost.crt").unwrap();
    // privkey_file.write_all(private_key.private_key_to_pem_pkcs8().unwrap().as_ref()).unwrap();
    // cert_file.write_all(cert.to_pem().unwrap().as_ref()).unwrap();
    // println!("Private key saved to: localhost.key");
    // println!("Certificate saved to: localhost.crt");

    // Return the certificate and private key
    Ok((cert, private_key))
}

// Make a X509 request with the given private key
fn mk_request(private_key: &PKey<Private>) -> Result<X509Req, ErrorStack> {

    //make the request
    let mut req = X509ReqBuilder::new()?;
    req.set_pubkey(private_key)?; // generate the public key from the private key

    // fill in the metadata
    let mut x509name = X509NameBuilder::new()?;
    x509name.append_entry_by_text("C", "HK")?; // country
    // x509name.append_entry_by_text("ST", "")?; // state
    x509name.append_entry_by_text("L", "Chai Wan")?; // locale(city)
    x509name.append_entry_by_text("O", "The Four-word mantra organization")?; // organization
    x509name.append_entry_by_text("CN", "DistinctlyLookingLovableMothers.com")?; // the (theoretic) domain of the authority
    let x509name = x509name.build(); // package the request
    req.set_subject_name(&x509name)?; // set issuer name as subject name

    req.sign(private_key, MessageDigest::sha256())?; // sign the request and hash it
    let req = req.build(); // package the request
    Ok(req) // return it
}

// Make a certificate and private key signed by the given CA cert and private key
fn mk_ca_signed_cert(
    ca_cert: &X509Ref,
    ca_key_pair: &PKeyRef<Private>,
) -> Result<(X509, PKey<Private>), ErrorStack> {

    // make a private key
    let private_key = PKey::from_rsa(Rsa::generate(2048)?)?;

    // get the request with the generated private key
    let req = mk_request(&private_key)?;

    // make the cert
    let mut cert = X509::builder()?;
    cert.set_version(3)?; // set the cert version
    let serial_number = { // make up the serial number which is just a number to identify the cert
        let mut serial = BigNum::new()?;
        serial.rand(159, MsbOption::MAYBE_ZERO, false)?;
        serial.to_asn1_integer()?
    };
    cert.set_serial_number(&serial_number)?; // set the serial number to the previously generated value
    cert.set_subject_name(req.subject_name())?; // set the subject name
    cert.set_issuer_name(ca_cert.subject_name())?; // set the issuer name (which is the organization name from the ca cert(i think))
    cert.set_pubkey(&private_key)?; // generate the public key from the given private key
    cert.set_not_before(&Asn1Time::days_from_now(0).unwrap())?; // set the start date to today
    cert.set_not_after(&Asn1Time::days_from_now(365).unwrap())?; // set the expiry date to one year later

    cert.append_extension(BasicConstraints::new().build()?)?; // flag the cert to be ca certified

    // prevents the key to be backupped and be used for any other purpose, in case someone got the key when theyre not supposed to
    // different from the line in mk_ca_cert() is that this one also requires a digital signature, flags the key is used to encrypt another cryptographic key,
    // and flags the authority unlikely to repudiate or deny that they back the cert, they could be lying but anyways
    // source: http://www.faqs.org/rfcs/rfc3280.html
    cert.append_extension(KeyUsage::new().critical().non_repudiation().digital_signature().key_encipherment().build()?,)?;

    // provides an identifier to the public key similar to fingerprint except baked into the cert itself, except this time we do have an authority (that we made up)
    cert.append_extension(SubjectKeyIdentifier::new().build(&cert.x509v3_context(Some(ca_cert), None))?)?;

    // provides a way to identify the authority signing the cert
    cert.append_extension(AuthorityKeyIdentifier::new().keyid(false).issuer(false).build(&cert.x509v3_context(Some(ca_cert), None))?)?;

    // allows us to bind more sites to the certificate
    cert.append_extension(SubjectAlternativeName::new().dns("*.example.com").dns("hello.com").dns("localhost:8080").build(&cert.x509v3_context(Some(ca_cert), None))?)?;

    cert.sign(ca_key_pair, MessageDigest::sha256())?; // sign the cert and hash it
    let cert = cert.build(); // package the cert

    Ok((cert, private_key)) // return the cert and the private key
}

pub(crate) fn generate_self_signed_cert() -> Result<(X509, PKey<Private>), ErrorStack> {

    // make the certification authority cert and certification authority private key
    let (ca_cert, ca_private_key) = mk_ca_cert()?;

    // make a cert certified by the authority by providing the authority cert and its private key
    let (cert, private_key) = mk_ca_signed_cert(&ca_cert, &ca_private_key)?;

    // Verify that this cert was issued by this certification authority
    match ca_cert.issued(&cert) {
        X509VerifyResult::OK => debug!("Certificate verified!"),
        ver_err => error!("Failed to verify certificate: {}", ver_err),
    };

    Ok((cert, private_key))

}