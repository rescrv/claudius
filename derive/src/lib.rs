#![recursion_limit = "128"]

extern crate proc_macro;
#[macro_use]
extern crate quote;
extern crate syn;

use proc_macro2::TokenStream;
use syn::{parse_macro_input, DeriveInput};

use derive_util::StructVisitor;

////////////////////////////////////// #[derive(CommandLine)] ///////////////////////////////////

/// Derive the CommandLine trait for a given struct.
#[proc_macro_derive(JsonSchema, attributes())]
pub fn derive_json_schema(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    // `ty_name` holds the type's identifier.
    let ty_name = input.ident;
    // Break out for templating purposes.
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let data = match input.data {
        syn::Data::Struct(ref ds) => ds,
        syn::Data::Enum(_) => {
            panic!("enums are not supported");
        }
        syn::Data::Union(_) => {
            panic!("unions are not supported");
        }
    };

    let mut jsv = JsonSchemaVisitor;
    let (value, required) = jsv.visit_struct(&ty_name, data);

    let gen = quote! {
        impl #impl_generics ::yammer::JsonSchema for #ty_name #ty_generics #where_clause {
            fn json_schema() -> serde_json::Value {
                let mut result = serde_json::json!{{}};
                let mut properties = serde_json::json!{{}};
                #value
                result["required"] = serde_json::Value::Array(vec![].into());
                #required
                result["type"] = "object".into();
                result["properties"] = properties;
                result
            }
        }
    };
    gen.into()
}

///////////////////////////////////////// JsonSchemaVisitor ////////////////////////////////////////

struct JsonSchemaVisitor;

impl StructVisitor for JsonSchemaVisitor {
    type Output = (TokenStream, TokenStream);

    fn visit_struct_named_fields(
        &mut self,
        _ty_name: &syn::Ident,
        _ds: &syn::DataStruct,
        fields: &syn::FieldsNamed,
    ) -> Self::Output {
        let mut result = quote! {};
        let mut required = quote! {};
        for field in fields.named.iter() {
            if let Some(field_ident) = &field.ident {
                let field_ident = field_ident.to_string();
                let field_ident = if let Some(field_ident) = field_ident.strip_prefix("r#") {
                    field_ident.to_string()
                } else {
                    field_ident.clone()
                };
                let field_type = field.ty.clone();
                result = quote! {
                    #result
                    properties[#field_ident] = <#field_type as ::yammer::JsonSchema>::json_schema();
                };
                required = quote! {
                    #required
                    if let Some(serde_json::Value::Array(arr)) = result.get_mut("required") {
                        arr.push(#field_ident.into())
                    }
                };
            }
        }
        (result, required)
    }
}
