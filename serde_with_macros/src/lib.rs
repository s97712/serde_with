#![deny(
    missing_copy_implementations,
    missing_debug_implementations,
    missing_docs,
    trivial_casts,
    trivial_numeric_casts,
    unused_extern_crates,
    unused_import_braces,
    unused_qualifications,
    variant_size_differences
)]
#![warn(rust_2018_idioms)]
#![doc(test(attr(deny(
    missing_copy_implementations,
    missing_debug_implementations,
    trivial_casts,
    trivial_numeric_casts,
    unused_extern_crates,
    unused_import_braces,
    unused_qualifications,
    variant_size_differences,
))))]
#![doc(test(attr(warn(rust_2018_idioms))))]
// Not needed for 2018 edition and conflicts with `rust_2018_idioms`
#![doc(test(no_crate_inject))]
#![doc(html_root_url = "https://docs.rs/serde_with_macros/1.2.0-alpha.3")]

//! proc-macro extensions for [`serde_with`]
//!
//! This crate should not be used alone, but through the [`serde_with`] crate.
//!
//! [`serde_with`]: https://crates.io/crates/serde_with/

#[allow(unused_extern_crates)]
extern crate proc_macro;

mod utils;

use crate::utils::IteratorExt as _;
use darling::{Error as DarlingError, FromField};
use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::{quote, ToTokens};
use std::str::FromStr;
use syn::{
    parse::Parser, punctuated::Pair, spanned::Spanned, Attribute, DeriveInput, Error, Field,
    Fields, GenericArgument, ItemEnum, ItemStruct, Meta, NestedMeta, Path, PathArguments,
    ReturnType, Type,
};

/// Apply function on every field of structs or enums
fn apply_function_to_struct_and_enum_fields<F>(
    input: TokenStream,
    function: F,
) -> Result<proc_macro2::TokenStream, Error>
where
    F: Copy,
    F: Fn(&mut Field) -> Result<(), String>,
{
    /// Handle a single struct or a single enum variant
    fn apply_on_fields<F>(fields: &mut Fields, function: F) -> Result<(), Error>
    where
        F: Fn(&mut Field) -> Result<(), String>,
    {
        match fields {
            // simple, no fields, do nothing
            Fields::Unit => Ok(()),
            Fields::Named(ref mut fields) => fields
                .named
                .iter_mut()
                .map(|field| function(field).map_err(|err| Error::new(field.span(), err)))
                .collect_error(),
            Fields::Unnamed(ref mut fields) => fields
                .unnamed
                .iter_mut()
                .map(|field| function(field).map_err(|err| Error::new(field.span(), err)))
                .collect_error(),
        }
    }

    // For each field in the struct given by `input`, add the `skip_serializing_if` attribute,
    // if and only if, it is of type `Option`
    if let Ok(mut input) = syn::parse::<ItemStruct>(input.clone()) {
        apply_on_fields(&mut input.fields, function)?;
        Ok(quote!(#input))
    } else if let Ok(mut input) = syn::parse::<ItemEnum>(input) {
        input
            .variants
            .iter_mut()
            .map(|variant| apply_on_fields(&mut variant.fields, function))
            .collect_error()?;
        Ok(quote!(#input))
    } else {
        Err(Error::new(
            Span::call_site(),
            "The attribute can only be applied to struct or enum definitions.",
        ))
    }
}

/// Like [apply_function_to_struct_and_enum_fields] but for darling errors
fn apply_function_to_struct_and_enum_fields_darling<F>(
    input: TokenStream,
    function: F,
) -> Result<proc_macro2::TokenStream, DarlingError>
where
    F: Copy,
    F: Fn(&mut Field) -> Result<(), DarlingError>,
{
    /// Handle a single struct or a single enum variant
    fn apply_on_fields<F>(fields: &mut Fields, function: F) -> Result<(), DarlingError>
    where
        F: Fn(&mut Field) -> Result<(), DarlingError>,
    {
        match fields {
            // simple, no fields, do nothing
            Fields::Unit => Ok(()),
            Fields::Named(ref mut fields) => {
                let errors: Vec<DarlingError> = fields
                    .named
                    .iter_mut()
                    .map(|field| function(field).map_err(|err| err.with_span(&field)))
                    // turn the Err variant into the Some, such that we only collect errors
                    .filter_map(|res| match res {
                        Err(e) => Some(e),
                        Ok(()) => None,
                    })
                    .collect();
                if errors.is_empty() {
                    Ok(())
                } else {
                    Err(DarlingError::multiple(errors))
                }
            }
            Fields::Unnamed(ref mut fields) => {
                let errors: Vec<DarlingError> = fields
                    .unnamed
                    .iter_mut()
                    .map(|field| function(field).map_err(|err| err.with_span(&field)))
                    // turn the Err variant into the Some, such that we only collect errors
                    .filter_map(|res| match res {
                        Err(e) => Some(e),
                        Ok(()) => None,
                    })
                    .collect();
                if errors.is_empty() {
                    Ok(())
                } else {
                    Err(DarlingError::multiple(errors))
                }
            }
        }
    }

    // For each field in the struct given by `input`, add the `skip_serializing_if` attribute,
    // if and only if, it is of type `Option`
    if let Ok(mut input) = syn::parse::<ItemStruct>(input.clone()) {
        apply_on_fields(&mut input.fields, function)?;
        Ok(quote!(#input))
    } else if let Ok(mut input) = syn::parse::<ItemEnum>(input) {
        let errors: Vec<DarlingError> = input
            .variants
            .iter_mut()
            .map(|variant| apply_on_fields(&mut variant.fields, function))
            // turn the Err variant into the Some, such that we only collect errors
            .filter_map(|res| match res {
                Err(e) => Some(e),
                Ok(()) => None,
            })
            .collect();
        if errors.is_empty() {
            Ok(quote!(#input))
        } else {
            Err(DarlingError::multiple(errors))
        }
    } else {
        Err(DarlingError::custom(
            "The attribute can only be applied to struct or enum definitions.",
        )
        .with_span(&Span::call_site()))
    }
}

/// Add `skip_serializing_if` annotations to [`Option`] fields.
///
/// The attribute can be added to structs and enums.
///
/// Import this attribute with `use serde_with::skip_serializing_none;`.
///
/// # Example
///
/// JSON APIs sometimes have many optional values.
/// Missing values should not be serialized, to keep the serialized format smaller.
/// Such a data type might look like:
///
/// ```rust
/// # use serde::Serialize;
/// #
/// #[derive(Serialize)]
/// struct Data {
///     #[serde(skip_serializing_if = "Option::is_none")]
///     a: Option<String>,
///     #[serde(skip_serializing_if = "Option::is_none")]
///     b: Option<u64>,
///     #[serde(skip_serializing_if = "Option::is_none")]
///     c: Option<String>,
///     #[serde(skip_serializing_if = "Option::is_none")]
///     d: Option<bool>,
/// }
/// ```
///
/// The `skip_serializing_if` annotation is repetitive and harms readability.
/// Instead the same struct can be written as:
///
/// ```rust
/// # use serde::Serialize;
/// # use serde_with_macros::skip_serializing_none;
/// #[skip_serializing_none]
/// #[derive(Serialize)]
/// struct Data {
///     a: Option<String>,
///     b: Option<u64>,
///     c: Option<String>,
///     // Always serialize field d even if None
///     #[serialize_always]
///     d: Option<bool>,
/// }
/// ```
///
/// Existing `skip_serializing_if` annotations will not be altered.
///
/// If some values should always be serialized, then the `serialize_always` can be used.
///
/// # Limitations
///
/// The `serialize_always` cannot be used together with a manual `skip_serializing_if` annotations, as these conflict in their meaning.
/// A compile error will be generated if this occurs.
///
/// The `skip_serializing_none` only works if the type is called [`Option`], [`std::option::Option`], or [`core::option::Option`].
/// Type aliasing an [`Option`] and giving it another name, will cause this field to be ignored.
/// This cannot be supported, as proc-macros run before type checking, thus it is not possible to determine if a type alias refers to an [`Option`].
///
/// ```rust
/// # use serde::Serialize;
/// # use serde_with_macros::skip_serializing_none;
/// type MyOption<T> = Option<T>;
///
/// #[skip_serializing_none]
/// #[derive(Serialize)]
/// struct Data {
///     a: MyOption<String>, // This field will not be skipped
/// }
/// ```
///
/// Likewise, if you import a type and name it `Option`, the `skip_serializing_if` attributes will be added and compile errors will occur, if `Option::is_none` is not a valid function.
/// Here the function `Vec::is_none` does not exist and therefore the example fails to compile.
///
/// ```rust,compile_fail
/// # use serde::Serialize;
/// # use serde_with_macros::skip_serializing_none;
/// use std::vec::Vec as Option;
///
/// #[skip_serializing_none]
/// #[derive(Serialize)]
/// struct Data {
///     a: Option<String>,
/// }
/// ```
///
#[proc_macro_attribute]
pub fn skip_serializing_none(_args: TokenStream, input: TokenStream) -> TokenStream {
    let res = match apply_function_to_struct_and_enum_fields(
        input,
        skip_serializing_none_add_attr_to_field,
    ) {
        Ok(res) => res,
        Err(err) => err.to_compile_error(),
    };
    TokenStream::from(res)
}

/// Add the skip_serializing_if annotation to each field of the struct
fn skip_serializing_none_add_attr_to_field(field: &mut Field) -> Result<(), String> {
    if let Type::Path(path) = &field.ty {
        if is_std_option(&path.path) {
            let has_skip_serializing_if =
                field_has_attribute(&field, "serde", "skip_serializing_if");

            // Remove the `serialize_always` attribute
            let mut has_always_attr = false;
            field.attrs.retain(|attr| {
                let has_attr = attr.path.is_ident("serialize_always");
                has_always_attr |= has_attr;
                !has_attr
            });

            // Error on conflicting attributes
            if has_always_attr && has_skip_serializing_if {
                let mut msg = r#"The attributes `serialize_always` and `serde(skip_serializing_if = "...")` cannot be used on the same field"#.to_string();
                if let Some(ident) = &field.ident {
                    msg += ": `";
                    msg += &ident.to_string();
                    msg += "`";
                }
                msg += ".";
                return Err(msg);
            }

            // Do nothing if `skip_serializing_if` or `serialize_always` is already present
            if has_skip_serializing_if || has_always_attr {
                return Ok(());
            }

            // Add the `skip_serializing_if` attribute
            let attr_tokens = quote!(
                #[serde(skip_serializing_if = "Option::is_none")]
            );
            let parser = Attribute::parse_outer;
            let attrs = parser
                .parse2(attr_tokens)
                .expect("Static attr tokens should not panic");
            field.attrs.extend(attrs);
        } else {
            // Warn on use of `serialize_always` on non-Option fields
            let has_attr = field
                .attrs
                .iter()
                .any(|attr| attr.path.is_ident("serialize_always"));
            if has_attr {
                return Err(
                    "`serialize_always` may only be used on fields of type `Option`.".into(),
                );
            }
        }
    }
    Ok(())
}

/// Return `true`, if the type path refers to `std::option::Option`
///
/// Accepts
///
/// * `Option`
/// * `std::option::Option`, with or without leading `::`
/// * `core::option::Option`, with or without leading `::`
fn is_std_option(path: &Path) -> bool {
    (path.leading_colon.is_none() && path.segments.len() == 1 && path.segments[0].ident == "Option")
        || (path.segments.len() == 3
            && (path.segments[0].ident == "std" || path.segments[0].ident == "core")
            && path.segments[1].ident == "option"
            && path.segments[2].ident == "Option")
}

/// Determine if the `field` has an attribute with given `namespace` and `name`
///
/// On the example of
/// `#[serde(skip_serializing_if = "Option::is_none")]`
///
/// * `serde` is the outermost path, here namespace
/// * it contains a Meta::List
/// * which contains in another Meta a Meta::NameValue
/// * with the name being `skip_serializing_if`
#[allow(clippy::cmp_owned)]
fn field_has_attribute(field: &Field, namespace: &str, name: &str) -> bool {
    // On the example of
    // #[serde(skip_serializing_if = "Option::is_none")]
    //
    // `serde` is the outermost path, here namespace
    // it contains a Meta::List
    // which contains in another Meta a Meta::NameValue
    // with the name being `skip_serializing_if`

    for attr in &field.attrs {
        if attr.path.is_ident(namespace) {
            // Ignore non parsable attributes, as these are not important for us
            if let Ok(expr) = attr.parse_meta() {
                if let Meta::List(expr) = expr {
                    for expr in expr.nested {
                        if let NestedMeta::Meta(Meta::NameValue(expr)) = expr {
                            if let Some(ident) = expr.path.get_ident() {
                                if ident.to_string() == name {
                                    return true;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    false
}

/// Conveniece macro to use the [`serde_as`] system.
///
/// The [`serde_as`] system is designed as a more flexiable alterative to serde's with-annotation.
///
/// # Example
///
/// ```rust,ignore
/// use serde_with::{serde_as, DisplayFromStr};
/// use std::collections::HashMap;
///
/// #[serde_as]
/// #[derive(Serialize, Deserialize)]
/// struct Data {
///     /// Serialize into number
///     #[serde_as(as = "_")]
///     a: u32,
///
///     /// Serialize into String
///     #[serde_as(as = "DisplayFromStr")]
///     b: u32,
///
///     /// Serialize into a map from String to String
///     #[serde_as(as = "HashMap<DisplayFromStr, _>")]
///     c: Vec<(u32, String)>,
/// }
/// ```
///
/// # Limitations
///
/// This macro cannot be used outside of the [`serde_with`] crate, since it relies on types defined therein.
/// The [`serde_with`] crate has to be available in the root namespace under `::serde_with`.
///
/// [`serde_as`]: https://docs.rs/serde_with/1.5.0-alpha.2/serde_with/guide/index.html
/// [`serde_with`]: https://crates.io/crates/serde_with/
#[proc_macro_attribute]
pub fn serde_as(_args: TokenStream, input: TokenStream) -> TokenStream {
    // Convert any error message into a nice compiler error
    let res =
        match apply_function_to_struct_and_enum_fields_darling(input, serde_as_add_attr_to_field) {
            Ok(res) => res,
            Err(err) => err.write_errors(),
        };
    TokenStream::from(res)
}

/// Add the skip_serializing_if annotation to each field of the struct
fn serde_as_add_attr_to_field(field: &mut Field) -> Result<(), DarlingError> {
    #[derive(FromField, Debug)]
    #[darling(attributes(serde_as))]
    struct SerdeAsOptions {
        #[darling(rename = "as", map = "parse_type_from_string", default)]
        r#as: Option<Result<Type, Error>>,
        #[darling(map = "parse_type_from_string", default)]
        deserialize_as: Option<Result<Type, Error>>,
        #[darling(map = "parse_type_from_string", default)]
        serialize_as: Option<Result<Type, Error>>,
    }

    impl SerdeAsOptions {
        fn has_any_set(&self) -> bool {
            self.r#as.is_some() || self.deserialize_as.is_some() || self.serialize_as.is_some()
        }
    }

    #[derive(FromField, Debug)]
    #[darling(attributes(serde), allow_unknown_fields)]
    struct SerdeWithOptions {
        #[darling(default)]
        with: Option<String>,
        #[darling(default)]
        deserialize_with: Option<String>,
        #[darling(default)]
        serialize_with: Option<String>,
    }

    impl SerdeWithOptions {
        fn has_any_set(&self) -> bool {
            self.with.is_some() || self.deserialize_with.is_some() || self.serialize_with.is_some()
        }
    }

    let serde_as_options = SerdeAsOptions::from_field(field)?;
    let serde_with_options = SerdeWithOptions::from_field(field)?;

    // Find index of serde_as attribute
    let serde_as_idx = field.attrs.iter().enumerate().find_map(|(idx, attr)| {
        if attr.path.is_ident("serde_as") {
            Some(idx)
        } else {
            None
        }
    });
    if serde_as_idx.is_none() {
        return Ok(());
    }
    let serde_as_idx = serde_as_idx.expect("Just checked that the value is Some");

    // serde_as Attribute
    field.attrs.remove(serde_as_idx);

    // Check if there are any conflicting attributes
    if serde_as_options.has_any_set() && serde_with_options.has_any_set() {
        return Err(DarlingError::custom("Cannot combine `serde_as` with serde's `with`, `deserialize_with`, or `serialize_with`."));
    }

    if serde_as_options.r#as.is_some() && serde_as_options.deserialize_as.is_some() {
        return Err(DarlingError::custom("Cannot combine `as` with `deserialize_as`. Use `serialize_as` to specify different serialization code."));
    } else if serde_as_options.r#as.is_some() && serde_as_options.serialize_as.is_some() {
        return Err(DarlingError::custom("Cannot combine `as` with `serialize_as`. Use `deserialize_as` to specify different deserialization code."));
    }

    if let Some(Ok(type_)) = serde_as_options.r#as {
        let attr_tokens = proc_macro2::TokenStream::from_str(&*format!(
            r##"#[serde(with = "::serde_with::As::<{}>")]"##,
            replace_infer_type_with_type(type_, &syn::parse_str("::serde_with::Same").unwrap())
                .to_token_stream()
        ))
        .unwrap();
        let attrs = Attribute::parse_outer
            .parse2(attr_tokens)
            .expect("Static attr tokens should not panic");
        field.attrs.extend(attrs);
    }
    if let Some(Ok(type_)) = serde_as_options.deserialize_as {
        let attr_tokens = proc_macro2::TokenStream::from_str(&*format!(
            r##"#[serde(deserialize_with = "::serde_with::As::<{}>::deserialize")]"##,
            replace_infer_type_with_type(type_, &syn::parse_str("::serde_with::Same").unwrap())
                .to_token_stream()
        ))
        .unwrap();
        let attrs = Attribute::parse_outer
            .parse2(attr_tokens)
            .expect("Static attr tokens should not panic");
        field.attrs.extend(attrs);
    }
    if let Some(Ok(type_)) = serde_as_options.serialize_as {
        let attr_tokens = proc_macro2::TokenStream::from_str(&*format!(
            r##"#[serde(serialize_with = "::serde_with::As::<{}>::serialize")]"##,
            replace_infer_type_with_type(type_, &syn::parse_str("::serde_with::Same").unwrap())
                .to_token_stream()
        ))
        .unwrap();
        let attrs = Attribute::parse_outer
            .parse2(attr_tokens)
            .expect("Static attr tokens should not panic");
        field.attrs.extend(attrs);
    }

    Ok(())
}

/// Parse a [String][] and return a [Type][]
fn parse_type_from_string(s: String) -> Option<Result<Type, Error>> {
    Some(syn::parse_str(&*s))
}

/// Recursivly replace all occurences of `_` with `replacement` in a [Type][]
///
/// The [serde_as][macro@serde_as] macro allows to use the infer type, i.e., `_`, as shortcut for `::serde_with::As`.
/// This function replaces all occurences of the infer type with another type.
fn replace_infer_type_with_type(to_replace: Type, replacement: &Type) -> Type {
    match to_replace {
        // Base case
        // Replace the infer type with the actuall replacement type
        Type::Infer(_) => replacement.clone(),

        // Recursive cases
        // Iterate through all positions where a type could occur and recursivly call this function
        Type::Array(mut inner) => {
            *inner.elem = replace_infer_type_with_type(*inner.elem, replacement);
            Type::Array(inner)
        }
        Type::Group(mut inner) => {
            *inner.elem = replace_infer_type_with_type(*inner.elem, replacement);
            Type::Group(inner)
        }
        Type::Never(inner) => Type::Never(inner),
        Type::Paren(mut inner) => {
            *inner.elem = replace_infer_type_with_type(*inner.elem, replacement);
            Type::Paren(inner)
        }
        Type::Path(mut inner) => {
            match inner.path.segments.pop() {
                Some(Pair::End(mut t)) | Some(Pair::Punctuated(mut t, _)) => {
                    t.arguments = match t.arguments {
                        PathArguments::None => PathArguments::None,
                        PathArguments::AngleBracketed(mut inner) => {
                            // Iterate over the args between the angle brackets
                            inner.args = inner
                                .args
                                .into_iter()
                                .map(|generic_argument| match generic_argument {
                                    // replace types within the generics list, but leave other stuff like lifetimes untouched
                                    GenericArgument::Type(type_) => GenericArgument::Type(
                                        replace_infer_type_with_type(type_, replacement),
                                    ),
                                    ga => ga,
                                })
                                .collect();
                            PathArguments::AngleBracketed(inner)
                        }
                        PathArguments::Parenthesized(mut inner) => {
                            inner.inputs = inner
                                .inputs
                                .into_iter()
                                .map(|type_| replace_infer_type_with_type(type_, replacement))
                                .collect();
                            inner.output = match inner.output {
                                ReturnType::Type(arrow, mut type_) => {
                                    *type_ = replace_infer_type_with_type(*type_, replacement);
                                    ReturnType::Type(arrow, type_)
                                }
                                default => default,
                            };
                            PathArguments::Parenthesized(inner)
                        }
                    };
                    inner.path.segments.push(t);
                }
                None => {}
            }
            Type::Path(inner)
        }
        Type::Ptr(mut inner) => {
            *inner.elem = replace_infer_type_with_type(*inner.elem, replacement);
            Type::Ptr(inner)
        }
        Type::Reference(mut inner) => {
            *inner.elem = replace_infer_type_with_type(*inner.elem, replacement);
            Type::Reference(inner)
        }
        Type::Slice(mut inner) => {
            *inner.elem = replace_infer_type_with_type(*inner.elem, replacement);
            Type::Slice(inner)
        }
        Type::Tuple(mut inner) => {
            inner.elems = inner
                .elems
                .into_pairs()
                .map(|pair| match pair {
                    Pair::Punctuated(type_, p) => {
                        Pair::Punctuated(replace_infer_type_with_type(type_, replacement), p)
                    }
                    Pair::End(type_) => Pair::End(replace_infer_type_with_type(type_, replacement)),
                })
                .collect();
            Type::Tuple(inner)
        }

        // Pass unknown types or non-handleable types (e.g., bare Fn) without performing any replacements
        type_ => type_,
    }
}

/// Deserialize value by using it's [`FromStr`] implementation
///
/// This is an alternative way to implement `Deserialize` for types which also implement [`FromStr`] by deserializing the type from string.
/// Ensure that the struct/enum also implements [`FromStr`].
/// If the implementation is missing you will get a error message like
/// ```text
/// error[E0277]: the trait bound `Struct: std::str::FromStr` is not satisfied
/// ```
/// Additionally, `FromStr::Err` **must** implement [`Display`] as otherwise you will see a rather unhelpful error message
///
/// Serialization with [`Display`] is available with the matching [`SerializeDisplay`] derive.
///
/// # Example
///
/// ```rust,ignore
/// use std::str::FromStr;
///
/// #[derive(DeserializeFromStr)]
/// struct A {
///     a: u32,
///     b: bool,
/// }
///
/// impl FromStr for A {
///     type Err = String;
///
///     /// Parse a value like `123<>true`
///     fn from_str(s: &str) -> Result<Self, Self::Err> {
///         let mut parts = s.split("<>");
///         let number = parts
///             .next()
///             .ok_or_else(|| "Missing first value".to_string())?
///             .parse()
///             .map_err(|err: ParseIntError| err.to_string())?;
///         let bool = parts
///             .next()
///             .ok_or_else(|| "Missing second value".to_string())?
///             .parse()
///             .map_err(|err: ParseBoolError| err.to_string())?;
///         Ok(Self { a: number, b: bool })
///     }
/// }
///
/// let a: A = serde_json::from_str("\"159<>true\"").unwrap();
/// assert_eq!(A { a: 159, b: true }, a);
/// ```
///
/// [`Display`]: std::fmt::Display
/// [`FromStr`]: std::str::FromStr
#[proc_macro_derive(DeserializeFromStr)]
pub fn derive_deserialize_fromstr(item: TokenStream) -> TokenStream {
    // Convert any error message into a nice compiler error
    let res = match deserialize_fromstr(item) {
        Ok(res) => res,
        Err(err) => err.to_compile_error(),
    };
    TokenStream::from(res)
}

fn deserialize_fromstr(item: TokenStream) -> Result<proc_macro2::TokenStream, Error> {
    let input = syn::parse::<DeriveInput>(item)?;
    let ident = input.ident;
    Ok(quote! {
        #[automatically_derived]
        impl<'de> serde::Deserialize<'de> for #ident
        where
            Self: std::str::FromStr,
            <Self as FromStr>::Err: std::fmt::Display,
        {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct Helper<S>(std::marker::PhantomData<S>);

                impl<'de, S> serde::de::Visitor<'de> for Helper<S>
                where
                    S: FromStr,
                    <S as std::str::FromStr>::Err: std::fmt::Display,
                {
                    type Value = S;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "string")
                    }

                    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
                    where
                        E: serde::de::Error,
                    {
                        value.parse::<Self::Value>().map_err(serde::de::Error::custom)
                    }
                }

                deserializer.deserialize_str(Helper(std::marker::PhantomData))
            }
        }
    })
}

/// Serialize value by using it's [`Display`] implementation
///
/// This is an alternative way to implement `Serialize` for types which also implement [`Display`] by serializing the type as string.
/// Ensure that the struct/enum also implements [`Display`].
/// If the implementation is missing you will get a error message like
/// ```text
/// error[E0277]: `Struct` doesn't implement `std::fmt::Display`
/// ```
///
/// Deserialization with [`FromStr`] is available with the matching [`DeserializeFromStr`] derive.
///
/// # Example
///
/// ```rust,ignore
/// use std::fmt;
///
/// #[derive(SerializeDisplay)]
/// struct A {
///     a: u32,
///     b: bool,
/// }
///
/// impl fmt::Display for A {
///     fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
///         write!(f, "{}<>{}", self.a, self.b)
///     }
/// }
///
/// let a = A { a: 123, b: false };
/// assert_eq!(r#""123<>false""#, serde_json::to_string(&a).unwrap());
/// ```
///
/// [`Display`]: std::fmt::Display
/// [`FromStr`]: std::str::FromStr
#[proc_macro_derive(SerializeDisplay)]
pub fn derive_serialize_display(item: TokenStream) -> TokenStream {
    // Convert any error message into a nice compiler error
    let res = match serialize_display(item) {
        Ok(res) => res,
        Err(err) => err.to_compile_error(),
    };
    TokenStream::from(res)
}

fn serialize_display(item: TokenStream) -> Result<proc_macro2::TokenStream, Error> {
    let input = syn::parse::<DeriveInput>(item)?;
    let ident = input.ident;
    Ok(quote! {
        #[automatically_derived]
        impl<'de> serde::Serialize for #ident
        where
            Self: std::fmt::Display,
        {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                serializer.serialize_str(&self.to_string())
            }
        }
    })
}
