use proc_macro2::TokenStream;
use quote::{format_ident, quote, quote_spanned};
use syn::{
    FnArg, Ident, ItemFn, Type, parse2, punctuated::Punctuated, spanned::Spanned, token::Comma,
};

pub fn test(args: TokenStream, item: TokenStream) -> TokenStream {
    let item_span = item.span();
    let args = inject_seed_config(args);
    let Ok(func) = parse2::<ItemFn>(item) else {
        return quote_spanned! { item_span =>
            compile_error!("#[gpui::property_test] must be placed on a function");
        };
    };

    let test_name = func.sig.ident.clone();
    let inner_fn_name = format_ident!("__{test_name}");
    let outer_fn_attributes = &func.attrs;

    let parsed_args = parse_args(func.sig.inputs, &test_name);

    let inner_body = func.block;
    let inner_arg_decls = parsed_args.inner_fn_decl_args;
    let asyncness = func.sig.asyncness;

    let inner_fn = quote! {
        let #inner_fn_name = #asyncness move |#inner_arg_decls| #inner_body;
    };

    let arg_errors = parsed_args.errors;
    let proptest_args = parsed_args.proptest_args;
    let inner_args = parsed_args.inner_fn_args;
    let cx_vars = parsed_args.cx_vars;
    let cx_teardowns = parsed_args.cx_teardowns;

    let proptest_args = quote! {
        #[strategy = ::gpui::seed_strategy()] __seed: u64,
        #proptest_args
    };

    let run_test_body = match &asyncness {
        None => quote! {
            #cx_vars
            #inner_fn_name(#inner_args);
            #cx_teardowns
        },
        Some(_) => quote! {
            let foreground_executor = gpui::ForegroundExecutor::new(std::sync::Arc::new(dispatcher.clone()));
            #cx_vars
            foreground_executor.block_test(#inner_fn_name(#inner_args));
            #cx_teardowns
        },
    };

    quote! {
        #arg_errors

        #[::gpui::proptest::property_test(proptest_path = "::gpui::proptest", #args)]
        #(#outer_fn_attributes)*
        fn #test_name(#proptest_args) {
            #inner_fn

            ::gpui::run_test_once(
                __seed,
                Box::new(move |dispatcher| {
                    #run_test_body
                }),
            )
        }
    }
}

fn inject_seed_config(args: TokenStream) -> TokenStream {
    use proc_macro2::TokenTree;

    let mut segments: Vec<Vec<TokenTree>> = vec![vec![]];
    for tt in args {
        match &tt {
            TokenTree::Punct(p) if p.as_char() == ',' => {
                segments.push(vec![]);
            }
            _ => {
                segments
                    .last_mut()
                    .expect("segments should never be empty")
                    .push(tt);
            }
        }
    }
    segments.retain(|s| !s.is_empty());

    let mut found_config = false;
    let mut result_segments: Vec<TokenStream> = Vec::new();

    for segment in segments {
        let is_config = segment
            .first()
            .is_some_and(|tt| matches!(tt, TokenTree::Ident(ident) if *ident == "config"));

        if is_config {
            found_config = true;
            let expr_tokens: TokenStream = segment.into_iter().skip(2).collect();
            result_segments.push(quote!(config = ::gpui::apply_seed_to_config(#expr_tokens)));
        } else {
            let segment_stream: TokenStream = segment.into_iter().collect();
            result_segments.push(segment_stream);
        }
    }

    if !found_config {
        result_segments.push(quote!(config = ::gpui::default_proptest_config()));
    }

    let mut result = TokenStream::new();
    for (i, segment) in result_segments.iter().enumerate() {
        if i > 0 {
            result.extend(quote!(,));
        }
        result.extend(segment.clone());
    }

    result
}

#[derive(Default)]
struct ParsedArgs {
    cx_vars: TokenStream,
    cx_teardowns: TokenStream,
    proptest_args: TokenStream,
    errors: TokenStream,

    // exprs passed at the call-site
    inner_fn_args: TokenStream,
    // args in the declaration
    inner_fn_decl_args: TokenStream,
}

fn parse_args(args: Punctuated<FnArg, Comma>, test_name: &Ident) -> ParsedArgs {
    let mut parsed = ParsedArgs::default();
    let mut args = args.into_iter().collect();

    remove_cxs(&mut parsed, &mut args, test_name);
    remove_std_rng(&mut parsed, &mut args);
    remove_background_executor(&mut parsed, &mut args);

    // all remaining args forwarded to proptest's macro
    parsed.proptest_args = quote!( #(#args),* );

    parsed
}

fn remove_cxs(parsed: &mut ParsedArgs, args: &mut Vec<FnArg>, test_name: &Ident) {
    let mut ix = 0;
    args.retain_mut(|arg| {
        if !is_test_cx(arg) {
            return true;
        }

        let cx_varname = format_ident!("cx_{ix}");
        ix += 1;

        parsed.cx_vars.extend(quote!(
            let mut #cx_varname = gpui::TestAppContext::build(
                dispatcher.clone(),
                Some(stringify!(#test_name)),
            );
        ));
        parsed.cx_teardowns.extend(quote!(
            dispatcher.run_until_parked();
            #cx_varname.executor().forbid_parking();
            #cx_varname.quit();
            dispatcher.run_until_parked();
        ));

        parsed.inner_fn_decl_args.extend(quote!(#arg,));
        parsed.inner_fn_args.extend(quote!(&mut #cx_varname,));

        false
    });
}

fn remove_std_rng(parsed: &mut ParsedArgs, args: &mut Vec<FnArg>) {
    args.retain_mut(|arg| {
        if !is_std_rng(arg) {
            return true;
        }

        parsed.errors.extend(quote_spanned! { arg.span() =>
            compile_error!("`StdRng` is not allowed in a property test. Consider implementing `Arbitrary`, or implementing a custom `Strategy`. https://altsysrq.github.io/proptest-book/proptest/tutorial/strategy-basics.html");
        });

        false
    });
}

fn remove_background_executor(parsed: &mut ParsedArgs, args: &mut Vec<FnArg>) {
    args.retain_mut(|arg| {
        if !is_background_executor(arg) {
            return true;
        }

        parsed.inner_fn_decl_args.extend(quote!(#arg,));
        parsed
            .inner_fn_args
            .extend(quote!(gpui::BackgroundExecutor::new(std::sync::Arc::new(
                dispatcher.clone()
            )),));

        false
    });
}

// Matches `&TestAppContext` or `&foo::bar::baz::TestAppContext`
fn is_test_cx(arg: &FnArg) -> bool {
    let FnArg::Typed(arg) = arg else {
        return false;
    };

    let Type::Reference(ty) = &*arg.ty else {
        return false;
    };

    let Type::Path(ty) = &*ty.elem else {
        return false;
    };

    ty.path
        .segments
        .last()
        .is_some_and(|seg| seg.ident == "TestAppContext")
}

fn is_std_rng(arg: &FnArg) -> bool {
    is_path_with_last_segment(arg, "StdRng")
}

fn is_background_executor(arg: &FnArg) -> bool {
    is_path_with_last_segment(arg, "BackgroundExecutor")
}

fn is_path_with_last_segment(arg: &FnArg, last_segment: &str) -> bool {
    let FnArg::Typed(arg) = arg else {
        return false;
    };

    let Type::Path(ty) = &*arg.ty else {
        return false;
    };

    ty.path
        .segments
        .last()
        .is_some_and(|seg| seg.ident == last_segment)
}
