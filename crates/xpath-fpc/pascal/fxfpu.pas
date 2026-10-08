unit fxfpu;

{ The float environment of a call into internettools.

  FMD2 evaluates XPath on FPC threads, which start with FPC's defaults: overflow, divide-by-zero and invalid operation
  unmasked. A float operation that hits one of them raises a Pascal exception (EOverflow, EZeroDivide, EInvalidOp),
  which internettools sometimes catches itself (data/xquery_schemas.inc, data/xquery__functions.pas) and which
  TXQueryEngineHTML.Eval otherwise turns into an empty value (baseunits/XQueryEngineHTML.pas:255-283). So
  `1e308 * 10` is empty in FMD2, not INF.

  To match that, every ABI call runs between EnterFpc and LeaveFpc: they switch to FPC's default environment and
  back to the host's, which masks every float exception. An FPC program turns the resulting SIGFPE into an exception
  with its own signal handler; a library installs none, so this unit installs a process-wide SIGFPE handler that does
  the same for a thread inside an ABI call and forwards every other SIGFPE to the handler that was there before. }

{$mode objfpc}{$H+}

interface

type
  TFpuEnv = record
    CW: Word;
    Pad: Word;
    MXCSR: DWord;
  end;
  PFpuEnv = ^TFpuEnv;

{ Switches the calling thread to FPC's default float environment and returns the caller's. }
function EnterFpc: TFpuEnv;
{ Restores the caller's float environment. }
procedure LeaveFpc(Env: TFpuEnv);

implementation

uses
  BaseUnix, UnixType, pthreads;

const
  { FPC's x86_64 defaults for a program and its threads. In a library FPC copies the host's environment into
    Default8087CW/DefaultMXCSR instead, so they are spelled out here. }
  FpcProgram8087CW = $1372;
  FpcProgramMXCSR = $1900;

var
  { Non-nil on a thread that is inside an ABI call. A pthread key rather than a threadvar, because the signal handler
    must not allocate, and FPC allocates a foreign thread's threadvars on first access. }
  InCallKey: pthread_key_t;
  PreviousSigFpe: SigActionRec;

procedure FpcDefaultSigHandler(sig: cint; info: PSigInfo; context: PSigContext); cdecl;
  external name '_FPC_DEFAULTSIGHANDLER';

procedure SigFpeHandler(sig: cint; info: PSigInfo; context: PSigContext); cdecl;
begin
  if pthread_getspecific(InCallKey) <> nil then
    FpcDefaultSigHandler(sig, info, context)
  else if (PreviousSigFpe.sa_flags and SA_SIGINFO) <> 0 then
    PreviousSigFpe.sa_handler(sig, info, context)
  else if (PreviousSigFpe.sa_handler = SigActionHandler(SIG_DFL)) or
    (PreviousSigFpe.sa_handler = SigActionHandler(SIG_IGN)) then
    // Not ours: put the previous disposition back; the faulting instruction runs again and gets it.
    fpsigaction(SIGFPE, @PreviousSigFpe, nil)
  else
    PreviousSigFpe.sa_handler(sig, nil, nil);
end;

procedure InstallSigFpeHandler;
var
  act: SigActionRec;
begin
  FillChar(act, SizeOf(act), 0);
  act.sa_handler := @SigFpeHandler;
  act.sa_flags := SA_SIGINFO;
  fpsigemptyset(act.sa_mask);
  fpsigaction(SIGFPE, @act, @PreviousSigFpe);
end;

{ The RTL's Set8087CW/SetMXCSR also overwrite the process-wide Default8087CW/DefaultMXCSR, which FPC loads into
  every thread it initializes and after every float exception; so the registers are read and written directly. }
procedure LoadFpuEnv(Env: PFpuEnv); assembler; nostackframe;
asm
  fnclex
  fldcw (%rdi)
  ldmxcsr 4(%rdi)
end;

procedure StoreFpuEnv(Env: PFpuEnv); assembler; nostackframe;
asm
  fnstcw (%rdi)
  stmxcsr 4(%rdi)
end;

function EnterFpc: TFpuEnv;
var
  fpc: TFpuEnv;
begin
  StoreFpuEnv(@Result);
  pthread_setspecific(InCallKey, Pointer(1));
  fpc.CW := FpcProgram8087CW;
  fpc.MXCSR := FpcProgramMXCSR;
  LoadFpuEnv(@fpc);
end;

procedure LeaveFpc(Env: TFpuEnv);
begin
  LoadFpuEnv(@Env);
  pthread_setspecific(InCallKey, nil);
end;

initialization
  pthread_key_create(@InCallKey, nil);
  InstallSigFpeHandler;
end.
