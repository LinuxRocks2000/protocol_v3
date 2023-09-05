use proc_macro::*;
use quote::quote;


#[proc_macro_derive(ProtocolFrame)]
pub fn protocol_frame_derive(input : TokenStream) -> TokenStream {
    let ast : syn::DeriveInput = syn::parse(input).unwrap();
    let name = ast.ident;
    match ast.data {
        syn::Data::Enum (enumdata) => {
            let mut encoder = vec![];
            let mut decoder = vec![];
            let mut identi : u8 = 0;
            for variant in &enumdata.variants {
                let ident = &variant.ident;
                let mut argnames = vec![];
                let mut i = 0;
                for _ in &variant.fields {
                    argnames.push(format!("a{}", i));
                    i += 1;
                };
                let thang = if variant.fields.len() == 0 { quote!{} } else {
                    let argstring = argnames.join(", ");
                    (format!("({})", argstring)).parse().unwrap()
                };
                let argnames_tss = argnames.into_iter().map(|x| {
                    x.parse::<proc_macro2::TokenStream>().unwrap()
                });
                encoder.push(quote! {
                    #name::#ident #thang => {
                        ret.push(#identi);
                        #(
                            let mut x = protocol_v3::protocol::protocol_encode(#argnames_tss.clone());
                            ret.append(&mut x);
                        )*
                        ret
                    }
                });
                let thang = if variant.fields.len() == 0 { quote!{} } else {
                    let mut stuff = vec![];
                    for field in &variant.fields {
                        match &field.ty {
                            syn::Type::Path (p) => {
                                stuff.push(p.path.segments[0].ident.to_string().parse::<proc_macro2::TokenStream>().unwrap());
                            }
                            _ => {}
                        }
                    }
                    quote!{
                        (
                            #(
                                protocol_v3::protocol::protocol_decode::<#stuff>(&mut data)?,
                            )*
                        )
                    }
                };
                decoder.push(quote! {
                    Some(#identi) => {
                        Ok(#name::#ident #thang)
                    }
                });
                if identi == 255 {
                    panic!("At the moment, there is a hard cap of 255 frame types!");
                }
                identi += 1;
            }
            let mut manifest = "{\"protocol\":\"".to_string();
            manifest += &name.to_string();
            manifest += "\",\"operations\":[";
            let mut identi : u8 = 0;
            for variant in &enumdata.variants {
                manifest += "{\"name\": \"";
                manifest += &variant.ident.to_string();
                manifest += "\",\"opcode\":";
                manifest += &identi.to_string();
                manifest += ",\"args\":[";
                let mut j = 0;
                for field in &variant.fields {
                    manifest += "\"";
                    manifest += &match &field.ty {
                        syn::Type::Path (p) => p.path.segments[0].ident.to_string(),
                        _ => String::new()
                    };
                    manifest += "\"";
                    if j < variant.fields.len() - 1 {
                        manifest += ",";
                    }
                    j += 1;
                }
                manifest += "]}";
                if (identi as usize) < enumdata.variants.len() - 1 {
                    manifest += ",";
                }
                identi += 1;
            }
            manifest += "]}";
            quote! {
                impl ProtocolFrame for #name {
                    fn encode(&self) -> Vec<u8> {
                        let mut ret : Vec<u8> = Vec::new();
                        match self {
                            #(
                                #encoder
                            )*
                        }
                    }
                    fn decode(mut data : std::collections::VecDeque<u8>) -> Result<#name, protocol_v3::protocol::DecodeError> {
                        match data.pop_front() {
                            #(
                                #decoder
                            )*
                            _ => {
                                Err(protocol_v3::protocol::DecodeError{})
                            }
                        }
                    }
                    fn manifest() -> &'static str {
                        #manifest
                    }
                }
            }
        },
        _ => {
            quote! {
                compile_error!("Only enums (not structs!) can be protocol frames")
            }
        },
    }.into()
}