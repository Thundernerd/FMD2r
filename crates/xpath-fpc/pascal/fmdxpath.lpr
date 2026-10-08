library fmdxpath;

{ libfmdxpath.so: FMD2's XPath engine (internettools, configured as in baseunits/XQueryEngineHTML.pas) over the C ABI
  in ../fmdxpath.h. }

{$mode objfpc}{$H+}

uses
  cthreads, // must come first: Rust threads call in, and FPC needs a thread manager for its threadvars and heap
  fxabi;

exports
  fx_doc_parse,
  fx_doc_free,
  fx_eval,
  fx_value_free,
  fx_value_count,
  fx_value_get,
  fx_value_to_string,
  fx_string_free,
  fx_last_error,
  fx_value_inner_html,
  fx_value_outer_html,
  fx_value_inner_text,
  fx_value_get_attribute,
  fx_value_get_property,
  fx_value_kind,
  fx_thread_exit;

begin
end.
