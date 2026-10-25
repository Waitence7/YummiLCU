using System;
using System.ComponentModel;
using System.IO;
using System.IO.Pipes;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;
using System.Threading.Tasks;
using Microsoft.Win32.SafeHandles;

namespace YummiGameBarWidget
{
    // Open the existing Agent pipe through the AppContainer-aware Windows API.
    // Keep handle creation out of the UWP runtime's legacy Connect path.
    internal static class NativePipeClient
    {
        [DllImport("api-ms-win-core-file-fromapp-l1-1-0.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        private static extern SafePipeHandle CreateFileFromAppW(
            string name, uint access, uint share, IntPtr security,
            uint creation, uint flags, IntPtr template);

        internal static NamedPipeClientStream Open(string pipeName, FileAccess access)
        {
            if (string.IsNullOrWhiteSpace(pipeName) || !pipeName.StartsWith(@"LOCAL\", StringComparison.Ordinal)
                || pipeName.Contains("..") || pipeName.Contains("/") || pipeName.IndexOf(':') >= 0)
                throw new ArgumentException("Expected a local Agent pipe name.", "pipeName");

            if (access != FileAccess.Read && access != FileAccess.ReadWrite)
                throw new ArgumentOutOfRangeException("access");
            uint desiredAccess = access == FileAccess.Read ? 0x80000000u : 0xC0000000u;
            // OVERLAPPED allows cancellable reads. SQOS_PRESENT + ANONYMOUS
            // keeps the pipe server from impersonating this client.
            var handle = CreateFileFromAppW(@"\\.\pipe\" + pipeName, desiredAccess, 3,
                IntPtr.Zero, 3, 0x40000000u | 0x00100000u, IntPtr.Zero);
            if (handle.IsInvalid)
            {
                int error = Marshal.GetLastWin32Error();
                handle.Dispose();
                throw new Win32Exception(error);
            }
            try { return new NamedPipeClientStream(access == FileAccess.Read ? PipeDirection.In : PipeDirection.InOut, true, true, handle); }
            catch { handle.Dispose(); throw; }
        }

        internal static async Task<string> ReadLineAsync(Stream stream, CancellationToken cancellationToken)
        {
            const int maximumBytes = 64 * 1024;
            var buffer = new byte[1024];
            using (cancellationToken.Register(() => stream.Dispose()))
            using (var message = new MemoryStream())
            {
                while (true)
                {
                    int count;
                    try { count = await stream.ReadAsync(buffer, 0, buffer.Length, cancellationToken).ConfigureAwait(false); }
                    catch { cancellationToken.ThrowIfCancellationRequested(); throw; }
                    cancellationToken.ThrowIfCancellationRequested();
                    if (count == 0) throw new EndOfStreamException("Agent closed an incomplete response.");
                    int newline = Array.IndexOf(buffer, (byte)'\n', 0, count);
                    int used = newline >= 0 ? newline : count;
                    if (message.Length + used > maximumBytes) throw new IOException("Agent response is too large.");
                    message.Write(buffer, 0, used);
                    if (newline >= 0)
                        return new UTF8Encoding(false, true).GetString(message.ToArray()).TrimEnd('\r');
                }
            }
        }

        internal static async Task<string> ReadSnapshotAsync()
        {
            using (var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(2)))
            using (var stream = Open(@"LOCAL\YummiLcuGameBarV1", FileAccess.Read))
                return await ReadLineAsync(stream, timeout.Token).ConfigureAwait(false);
        }
    }
}
