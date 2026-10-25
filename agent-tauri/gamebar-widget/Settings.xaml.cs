using System;
using System.IO;
using System.IO.Pipes;
using System.Text;
using System.Threading.Tasks;
using Microsoft.Gaming.XboxGameBar;
using Windows.Data.Json;
using Windows.UI.Xaml;
using Windows.UI.Xaml.Controls;
using Windows.UI.Xaml.Navigation;

namespace YummiGameBarWidget
{
    public sealed partial class Settings : Page
    {
        private const string CommandPipeName = @"LOCAL\YummiLcuGameBarCommandV1";
        private XboxGameBarWidget widget;

        public Settings() { InitializeComponent(); }

        protected override void OnNavigatedTo(NavigationEventArgs e)
        {
            widget = e.Parameter as XboxGameBarWidget;
            base.OnNavigatedTo(e);
        }

        private async void CheckUpdateButton_Click(object sender, RoutedEventArgs e)
        {
            CheckUpdateButton.IsEnabled = false;
            UpdateStatusText.Text = "업데이트 확인 요청 중…";
            try
            {
                var response = await SendCommandAsync("{\"command\":\"check_overlay_update\"}");
                var json = JsonObject.Parse(response);
                var ok = json.GetNamedBoolean("ok", false);
                UpdateStatusText.Text = json.GetNamedString("message", ok ? "업데이트 확인을 시작했습니다." : "업데이트 확인에 실패했습니다.");
            }
            catch
            {
                UpdateStatusText.Text = "Yummi LCU Agent에 연결하지 못했습니다. Agent를 실행해 주세요.";
            }
            finally
            {
                CheckUpdateButton.IsEnabled = true;
            }
        }

        private static async Task<string> SendCommandAsync(string payload)
        {
            using (var pipe = new NamedPipeClientStream(".", CommandPipeName, PipeDirection.InOut, PipeOptions.Asynchronous))
            {
                await pipe.ConnectAsync(1000);
                using (var writer = new StreamWriter(pipe, new UTF8Encoding(false), 1024, true) { AutoFlush = true })
                using (var reader = new StreamReader(pipe, Encoding.UTF8, false, 4096, true))
                {
                    await writer.WriteLineAsync(payload);
                    var line = await reader.ReadLineAsync();
                    if (string.IsNullOrWhiteSpace(line)) throw new IOException("empty command response");
                    return line;
                }
            }
        }
    }
}
