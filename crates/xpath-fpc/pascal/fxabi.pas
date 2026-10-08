unit fxabi;

{ The C ABI declared in ../fmdxpath.h, over FMD2's TXQueryEngineHTML set-up (baseunits/XQueryEngineHTML.pas).

  No Pascal exception may cross the ABI: every exported function catches everything and records the message in a
  per-thread last error (fx_last_error). }

{$mode objfpc}{$H+}

interface

uses
  SysUtils, xquery, xquery_json, simplehtmltreeparser, fxfpu;

type
  PFxDoc = ^TFxDoc;

  { A parsed document: FMD2's TXQueryEngineHTML (engine + tree parser). Reference counted, because every value keeps
    its document alive: nodes in a value point into the parser's trees. }
  TFxDoc = record
    Refs: LongInt;
    Engine: TXQueryEngine;
    TreeParser: TTreeParser;
  end;

  PFxValue = ^TFxValue;

  { A byte string owned by the caller, freed with fx_string_free. }
  TFxString = record
    ptr: PChar;
    len: SizeUInt;
  end;

  TFxValue = record
    Doc: PFxDoc;
    Value: IXQValue;
  end;

function fx_doc_parse(html: PChar; len: SizeUInt): PFxDoc; cdecl;
procedure fx_doc_free(doc: PFxDoc); cdecl;
function fx_eval(doc: PFxDoc; expr: PChar; len: SizeUInt; context: PFxValue; is_css: LongInt): PFxValue; cdecl;
procedure fx_value_free(v: PFxValue); cdecl;
function fx_value_count(v: PFxValue): Int64; cdecl;
function fx_value_get(v: PFxValue; i: Int64): PFxValue; cdecl;
function fx_value_to_string(v: PFxValue): TFxString; cdecl;
procedure fx_string_free(s: TFxString); cdecl;
function fx_last_error: TFxString; cdecl;
function fx_value_inner_html(v: PFxValue): TFxString; cdecl;
function fx_value_outer_html(v: PFxValue): TFxString; cdecl;
function fx_value_inner_text(v: PFxValue): TFxString; cdecl;
function fx_value_get_attribute(v: PFxValue; name: PChar; len: SizeUInt): TFxString; cdecl;
function fx_value_get_property(v: PFxValue; name: PChar; len: SizeUInt): PFxValue; cdecl;
function fx_value_kind(v: PFxValue): LongInt; cdecl;
procedure fx_thread_exit; cdecl;

implementation

threadvar
  LastError: String;

procedure SetLastError(E: TObject);
begin
  if E is Exception then
    LastError := E.ClassName + ': ' + Exception(E).Message
  else if Assigned(E) then
    LastError := E.ClassName
  else
    LastError := 'unknown error';
end;

function PasString(p: PChar; len: SizeUInt): String;
begin
  if (p = nil) and (len > 0) then
    raise Exception.Create('NULL string with a non-zero length');
  SetString(Result, p, len);
end;

function NewFxString(const s: String): TFxString;
begin
  Result.ptr := nil;
  Result.len := Length(s);
  if Result.len > 0 then
  begin
    Result.ptr := GetMem(Result.len);
    Move(PChar(s)^, Result.ptr^, Result.len);
  end;
end;

procedure DocAddRef(doc: PFxDoc);
begin
  if doc <> nil then
    InterlockedIncrement(doc^.Refs);
end;

procedure DocRelease(doc: PFxDoc);
begin
  if (doc <> nil) and (InterlockedDecrement(doc^.Refs) = 0) then
  begin
    // Same order as TXQueryEngineHTML.Destroy (baseunits/XQueryEngineHTML.pas:410-415).
    doc^.Engine.Free;
    doc^.TreeParser.Free;
    Dispose(doc);
  end;
end;

{ A value handle; doc is nil for the empty value that a failed call returns. }
function NewValue(doc: PFxDoc; const v: IXQValue): PFxValue;
begin
  New(Result);
  DocAddRef(doc);
  Result^.Doc := doc;
  Result^.Value := v;
end;

{ Records E and returns an empty value, so value-returning calls never return NULL (barring out-of-memory). }
function Failed(E: TObject): PFxValue;
begin
  SetLastError(E);
  try
    Result := NewValue(nil, xqvalue());
  except
    Result := nil;
  end;
end;

function DocOf(doc: PFxDoc): PFxDoc;
begin
  if doc = nil then
    raise Exception.Create('NULL fx_doc');
  Result := doc;
end;

function ValueOf(v: PFxValue): IXQValue;
begin
  if v = nil then
    raise Exception.Create('NULL fx_value');
  Result := v^.Value;
end;

{ IXQValue.toNode. FMD2 dereferences the nil it returns for a non-node (an access violation); here that is an error. }
function NodeOf(v: PFxValue): TTreeNode;
begin
  Result := ValueOf(v).toNode;
  if Result = nil then
    raise Exception.Create('value is not a node');
end;

function fx_doc_parse(html: PChar; len: SizeUInt): PFxDoc; cdecl;
var
  fpu: TFpuEnv;
begin
  Result := nil;
  fpu := EnterFpc;
  try
    New(Result);
    Result^.Refs := 1;
    Result^.Engine := nil;
    Result^.TreeParser := nil;
    // TXQueryEngineHTML.Create, line by line (baseunits/XQueryEngineHTML.pas:384-400).
    Result^.Engine := TXQueryEngine.Create;
    Result^.TreeParser := TTreeParser.Create;
    with Result^.TreeParser do
    begin
      parsingModel := pmHTML;
      repairMissingStartTags := True;
      repairMissingEndTags := True;
      trimText := False;
      readComments := False;
      readProcessingInstructions := False;
      autoDetectHTMLEncoding := False;
      if len > 0 then
        parseTree(PasString(html, len));
    end;
  except
    on E: TObject do
    begin
      SetLastError(E);
      try
        DocRelease(Result);
      except
      end;
      Result := nil;
    end;
  end;
  LeaveFpc(fpu);
end;

procedure fx_doc_free(doc: PFxDoc); cdecl;
var
  fpu: TFpuEnv;
begin
  fpu := EnterFpc;
  try
    DocRelease(doc);
  except
    on E: TObject do
      SetLastError(E);
  end;
  LeaveFpc(fpu);
end;

{ TXQueryEngineHTML.Eval (baseunits/XQueryEngineHTML.pas:252-284): an evaluation error yields an empty sequence. }
function fx_eval(doc: PFxDoc; expr: PChar; len: SizeUInt; context: PFxValue; is_css: LongInt): PFxValue; cdecl;
var
  expression: String;
  context_item: IXQValue;
  tree: TTreeNode;
  engine: TXQueryEngine;
  fpu: TFpuEnv;
begin
  fpu := EnterFpc;
  try
    LastError := '';
    expression := PasString(expr, len);
    engine := DocOf(doc)^.Engine;
    if context <> nil then
    begin
      context_item := context^.Value;
      if is_css <> 0 then
        Result := NewValue(doc, engine.evaluateCSS3(expression, context_item))
      else
        Result := NewValue(doc, engine.evaluateXPath(expression, context_item));
    end
    else
    begin
      tree := doc^.TreeParser.getLastTree;
      if is_css <> 0 then
        Result := NewValue(doc, engine.evaluateCSS3(expression, tree))
      else
        Result := NewValue(doc, engine.evaluateXPath(expression, tree));
    end;
  except
    on E: TObject do
      Result := Failed(E);
  end;
  LeaveFpc(fpu);
end;

procedure fx_value_free(v: PFxValue); cdecl;
var
  fpu: TFpuEnv;
begin
  if v = nil then
    Exit;
  fpu := EnterFpc;
  try
    DocRelease(v^.Doc);
    Dispose(v);
  except
    on E: TObject do
      SetLastError(E);
  end;
  LeaveFpc(fpu);
end;

{ IXQValue.Count (baseunits/lua/LuaIXQValue.pas:74-78). }
function fx_value_count(v: PFxValue): Int64; cdecl;
var
  fpu: TFpuEnv;
begin
  Result := 0;
  fpu := EnterFpc;
  try
    Result := ValueOf(v).Count;
  except
    on E: TObject do
      SetLastError(E);
  end;
  LeaveFpc(fpu);
end;

{ IXQValue.get (baseunits/lua/LuaIXQValue.pas:130): 1-based; out of range yields an empty value. }
function fx_value_get(v: PFxValue; i: Int64): PFxValue; cdecl;
var
  r: IXQValue;
  fpu: TFpuEnv;
begin
  fpu := EnterFpc;
  try
    r := ValueOf(v).get(i);
    Result := NewValue(v^.Doc, r);
  except
    on E: TObject do
      Result := Failed(E);
  end;
  LeaveFpc(fpu);
end;

{ IXQValue.kind as fmdxpath.h's fx_kind. Spelled out so a reordered TXQValueKind can't shift the ABI. }
function fx_value_kind(v: PFxValue): LongInt; cdecl;
var
  fpu: TFpuEnv;
begin
  Result := 0;
  fpu := EnterFpc;
  try
    case ValueOf(v).kind of
      pvkUndefined: Result := 0;
      pvkBoolean: Result := 1;
      pvkInt64: Result := 2;
      pvkNull: Result := 3;
      pvkNode: Result := 4;
      pvkSequence: Result := 5;
      pvkArray: Result := 6;
      pvkDouble: Result := 7;
      pvkString: Result := 8;
      pvkBigDecimal: Result := 9;
      pvkBinary: Result := 10;
      pvkQName: Result := 11;
      pvkDateTime: Result := 12;
      pvkObject: Result := 13;
      pvkFunction: Result := 14;
    end;
  except
    on E: TObject do
      SetLastError(E);
  end;
  LeaveFpc(fpu);
end;

{ IXQValue.toString, as LuaIXQValue's ToString (baseunits/lua/LuaIXQValue.pas:37-41). }
function fx_value_to_string(v: PFxValue): TFxString; cdecl;
var
  fpu: TFpuEnv;
begin
  Result := Default(TFxString);
  fpu := EnterFpc;
  try
    Result := NewFxString(ValueOf(v).toString);
  except
    on E: TObject do
      SetLastError(E);
  end;
  LeaveFpc(fpu);
end;

{ LuaIXQValue's InnerHTML (baseunits/lua/LuaIXQValue.pas:56-60). }
function fx_value_inner_html(v: PFxValue): TFxString; cdecl;
var
  fpu: TFpuEnv;
begin
  Result := Default(TFxString);
  fpu := EnterFpc;
  try
    Result := NewFxString(NodeOf(v).innerHTML());
  except
    on E: TObject do
      SetLastError(E);
  end;
  LeaveFpc(fpu);
end;

{ LuaIXQValue's OuterHTML (baseunits/lua/LuaIXQValue.pas:62-66). }
function fx_value_outer_html(v: PFxValue): TFxString; cdecl;
var
  fpu: TFpuEnv;
begin
  Result := Default(TFxString);
  fpu := EnterFpc;
  try
    Result := NewFxString(NodeOf(v).outerHTML());
  except
    on E: TObject do
      SetLastError(E);
  end;
  LeaveFpc(fpu);
end;

{ LuaIXQValue's InnerText (baseunits/lua/LuaIXQValue.pas:68-72). }
function fx_value_inner_text(v: PFxValue): TFxString; cdecl;
var
  fpu: TFpuEnv;
begin
  Result := Default(TFxString);
  fpu := EnterFpc;
  try
    Result := NewFxString(NodeOf(v).innerText());
  except
    on E: TObject do
      SetLastError(E);
  end;
  LeaveFpc(fpu);
end;

{ LuaIXQValue's GetAttribute (baseunits/lua/LuaIXQValue.pas:43-48): '' when the attribute is missing. }
function fx_value_get_attribute(v: PFxValue; name: PChar; len: SizeUInt): TFxString; cdecl;
var
  fpu: TFpuEnv;
begin
  Result := Default(TFxString);
  fpu := EnterFpc;
  try
    Result := NewFxString(NodeOf(v).getAttribute(PasString(name, len)));
  except
    on E: TObject do
      SetLastError(E);
  end;
  LeaveFpc(fpu);
end;

{ LuaIXQValue's GetProperty (baseunits/lua/LuaIXQValue.pas:50-54): an empty sequence for a missing property or a
  non-object. }
function fx_value_get_property(v: PFxValue; name: PChar; len: SizeUInt): PFxValue; cdecl;
var
  r: IXQValue;
  fpu: TFpuEnv;
begin
  fpu := EnterFpc;
  try
    r := ValueOf(v).getProperty(PasString(name, len));
    Result := NewValue(v^.Doc, r);
  except
    on E: TObject do
      Result := Failed(E);
  end;
  LeaveFpc(fpu);
end;

procedure fx_string_free(s: TFxString); cdecl;
begin
  try
    if s.ptr <> nil then
      FreeMem(s.ptr);
  except
    on E: TObject do
      SetLastError(E);
  end;
end;

{ The message of the last failure on the calling thread; empty after a successful fx_eval. }
function fx_last_error: TFxString; cdecl;
var
  fpu: TFpuEnv;
begin
  Result := Default(TFxString);
  fpu := EnterFpc;
  try
    Result := NewFxString(LastError);
  except
  end;
  LeaveFpc(fpu);
end;

{ Frees what the calling host thread keeps between calls: internettools' per-thread caches (xquery.freeThreadVars) and
  the last error. While either is allocated, the thread's FPC heap chunk (32 KiB) outlives the thread. FPC's own
  DoneThread is not called: it crashes on a thread that FPC did not create. }
procedure fx_thread_exit; cdecl;
var
  fpu: TFpuEnv;
begin
  fpu := EnterFpc;
  try
    xquery.freeThreadVars;
    LastError := '';
  except
  end;
  LeaveFpc(fpu);
end;

end.
