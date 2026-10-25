param([string]$Source = (Join-Path (Split-Path -Parent $PSScriptRoot) 'NativePipeClient.cs'))
$ErrorActionPreference = 'Stop'
$harness = @'
using System;
using System.IO;
using System.IO.Pipes;
using System.Text;
using System.Threading;
using System.Threading.Tasks;
namespace YummiGameBarWidget {
public static class BridgeTests {
  public static string ReadLiveSnapshot() {
    return NativePipeClient.ReadSnapshotAsync().GetAwaiter().GetResult();
  }
  public static void Run() { RunAsync().GetAwaiter().GetResult(); }
  private static async Task RunAsync() {
    int probes = 0;
    var fitted = await HudGeometry.FitLargestAsync(1440, 900, 640, 360, (w, h) => {
      probes++;
      return Task.FromResult(w <= 1435 && h <= 806);
    });
    if (!fitted.Accepted || fitted.Width != 1435 || fitted.Height != 806 || probes > 30)
      throw new Exception("Rejected fullscreen request must converge to the largest allowed bounds");
    probes = 0;
    var exact = await HudGeometry.FitLargestAsync(1440, 900, 640, 360, (w, h) => { probes++; return Task.FromResult(true); });
    if (!exact.Accepted || exact.Width != 1440 || exact.Height != 900 || probes != 1)
      throw new Exception("An accepted display size must not be probed again");
    var heightOnly = await HudGeometry.FitLargestAsync(1440, 900, 640, 360, (w, h) => Task.FromResult(h <= 806));
    if (!heightOnly.Accepted || heightOnly.Width != 1440 || heightOnly.Height != 806)
      throw new Exception("A height limit must not unnecessarily reduce width");
    var rejected = await HudGeometry.FitLargestAsync(1440, 900, 640, 360, (w, h) => Task.FromResult(false));
    if (rejected.Accepted) throw new Exception("Rejected minimum must not report fitting success");
    Console.WriteLine("PASS: bounded maximum fit, exact-size fast path, and rejected minimum");
    string name = @"LOCAL\YummiBridgeTest-" + Guid.NewGuid().ToString("N");
    using (var server = new NamedPipeServerStream(name, PipeDirection.Out, 1, PipeTransmissionMode.Byte, PipeOptions.Asynchronous)) {
      var connected = server.WaitForConnectionAsync();
      using (var client = NativePipeClient.Open(name, FileAccess.Read)) {
        await connected;
        string expected = "{\"connected\":true,\"text\":\"\uD55C\uAE00\"}";
        byte[] bytes = Encoding.UTF8.GetBytes(expected + "\n");
        var read = NativePipeClient.ReadLineAsync(client, CancellationToken.None);
        for (int i = 0; i < bytes.Length; i++) { await server.WriteAsync(bytes, i, 1); await server.FlushAsync(); }
        if (await read != expected) throw new Exception("Chunked UTF-8 frame was corrupted");
      }
    }
    Console.WriteLine("PASS: native pipe and chunked UTF-8 frame");
    name = @"LOCAL\YummiBridgeTimeout-" + Guid.NewGuid().ToString("N");
    using (var server = new NamedPipeServerStream(name, PipeDirection.Out, 1, PipeTransmissionMode.Byte, PipeOptions.Asynchronous)) {
      var connected = server.WaitForConnectionAsync();
      using (var client = NativePipeClient.Open(name, FileAccess.Read)) {
        await connected;
        using (var cancel = new CancellationTokenSource(100)) {
          bool cancelled = false;
          try { await NativePipeClient.ReadLineAsync(client, cancel.Token); }
          catch (OperationCanceledException) { cancelled = true; }
          if (!cancelled) throw new Exception("Silent server did not cancel");
        }
      }
    }
    bool oversized = false;
    using (var data = new MemoryStream(new byte[65537])) {
      try { await NativePipeClient.ReadLineAsync(data, CancellationToken.None); }
      catch (IOException) { oversized = true; }
    }
    if (!oversized) throw new Exception("Oversized response accepted");
    bool incomplete = false;
    using (var data = new MemoryStream(Encoding.UTF8.GetBytes("{}"))) {
      try { await NativePipeClient.ReadLineAsync(data, CancellationToken.None); }
      catch (EndOfStreamException) { incomplete = true; }
    }
    if (!incomplete) throw new Exception("Incomplete frame accepted");
  }
}
}
'@
$harness = [regex]::Replace($harness, '(?m)^using .*;\r?\n', '')
$geometry = Get-Content (Join-Path (Split-Path -Parent $Source) 'HudGeometry.cs') -Raw
$geometry = [regex]::Replace($geometry, '(?m)^using .*;\r?\n', '')
Add-Type -TypeDefinition ((Get-Content $Source -Raw) + "`n" + $geometry + "`n" + $harness)
[YummiGameBarWidget.BridgeTests]::Run()
Write-Output 'PASS: native pipe handle, chunked UTF-8, cancellation, frame size and incomplete response'
if ($env:YUMMI_TEST_LIVE_BRIDGE -eq '1') {
  $snapshot = [YummiGameBarWidget.BridgeTests]::ReadLiveSnapshot() | ConvertFrom-Json
  [pscustomobject]@{NativeBridge='Ok';Connected=$snapshot.connected;Phase=$snapshot.phase} | ConvertTo-Json -Compress
}
