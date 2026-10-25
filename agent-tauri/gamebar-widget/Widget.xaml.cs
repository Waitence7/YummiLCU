using System;
using System.Collections.Generic;
using System.IO;
using System.IO.Pipes;
using System.Linq;
using System.Text;
using System.Threading.Tasks;
using Microsoft.Gaming.XboxGameBar;
using Windows.Data.Json;
using Windows.Graphics.Display;
using Windows.Networking;
using Windows.Networking.Sockets;
using Windows.Storage.Streams;
using Windows.UI;
using Windows.UI.Xaml;
using Windows.UI.Xaml.Controls;
using Windows.UI.Xaml.Media;
using Windows.UI.Xaml.Navigation;

namespace YummiGameBarWidget
{
    public sealed partial class Widget : Page
    {
        private const string PipeName = @"LOCAL\YummiLcuGameBarV1";
        private readonly DispatcherTimer timer = new DispatcherTimer { Interval = TimeSpan.FromSeconds(1) };
        private XboxGameBarWidget widget;
        private bool refreshing;
        private string resizeState = "화면 크기 확인 중";

        public Widget()
        {
            InitializeComponent();
            timer.Tick += async (_, __) => await RefreshAsync();
            Loaded += async (_, __) =>
            {
                await FitToDisplayAsync();
                timer.Start();
                await RefreshAsync();
            };
            Unloaded += (_, __) => timer.Stop();
        }

        protected override void OnNavigatedTo(NavigationEventArgs e)
        {
            widget = e.Parameter as XboxGameBarWidget;
            base.OnNavigatedTo(e);
        }

        private async Task FitToDisplayAsync()
        {
            if (widget == null) return;
            try
            {
                var display = DisplayInformation.GetForCurrentView();
                var scale = Math.Max(1.0, display.RawPixelsPerViewPixel);
                var width = Math.Max(640, display.ScreenWidthInRawPixels / scale);
                var height = Math.Max(360, display.ScreenHeightInRawPixels / scale);
                var full = new Windows.Foundation.Size(width, height);
                widget.MinWindowSize = full;
                widget.MaxWindowSize = full;
                var resized = await widget.TryResizeWindowAsync(full);
                await widget.CenterWindowAsync();
                resizeState = resized
                    ? $"화면 맞춤 {Math.Round(width)}×{Math.Round(height)}"
                    : "Game Bar가 전체 화면 크기 요청을 제한함";
            }
            catch (Exception ex)
            {
                resizeState = "화면 맞춤 실패: " + ex.GetType().Name;
            }
        }

        private async Task RefreshAsync()
        {
            if (refreshing) return;
            refreshing = true;
            try
            {
                string line = null;
                try
                {
                    using (var pipe = new NamedPipeClientStream(".", PipeName, PipeDirection.In, PipeOptions.Asynchronous))
                    {
                        await pipe.ConnectAsync(250);
                        using (var reader = new StreamReader(pipe, Encoding.UTF8, false, 4096, false))
                            line = await reader.ReadLineAsync();
                    }
                }
                catch
                {
                    line = await ReadLoopbackSnapshotAsync();
                }

                if (string.IsNullOrWhiteSpace(line)) throw new IOException("empty bridge response");
                Apply(JsonObject.Parse(line));
            }
            catch (Exception ex) { SetOffline(ex); }
            finally { refreshing = false; }
        }


        private async Task<string> ReadLoopbackSnapshotAsync()
        {
            using (var socket = new StreamSocket())
            {
                await socket.ConnectAsync(new HostName("127.0.0.1"), "47658");
                using (var reader = new DataReader(socket.InputStream))
                {
                    reader.InputStreamOptions = InputStreamOptions.Partial;
                    var loaded = await reader.LoadAsync(16 * 1024);
                    if (loaded == 0) return null;
                    return reader.ReadString(reader.UnconsumedBufferLength).Trim();
                }
            }
        }

        private void Apply(JsonObject json)
        {
            bool connected = json.GetNamedBoolean("connected", false);
            string phase = json.GetNamedString("phase", connected ? "Unknown" : "Offline");
            string riotId = json.GetNamedString("riot_id", "");
            var party = new List<string>();
            if (json.ContainsKey("party_members") && json["party_members"].ValueType == JsonValueType.Array)
                foreach (var value in json.GetNamedArray("party_members")) if (value.ValueType == JsonValueType.String) party.Add(value.GetString());

            PhaseText.Text = phase;
            RiotIdText.Text = string.IsNullOrWhiteSpace(riotId) ? (connected ? "Riot ID 확인 중" : "League 클라이언트를 기다리는 중") : riotId;
            PartyText.Text = party.Count == 0 ? "파티 정보 없음" : string.Join(" · ", party.Take(5));
            BridgeText.Text = (connected ? "LCU 연결됨" : "유미 에이전트 연결됨 · League 대기 중") + " · " + resizeState;
            StatusDot.Fill = new SolidColorBrush(connected ? Color.FromArgb(255, 86, 221, 139) : Color.FromArgb(255, 122, 106, 134));
        }

        private void SetOffline(Exception error)
        {
            PhaseText.Text = "Offline";
            RiotIdText.Text = "유미 LCU Agent를 실행해 주세요";
            PartyText.Text = "파티 정보 없음";
            BridgeText.Text = "에이전트 연결 안 됨 · " + error.GetType().Name + " · " + resizeState;
            StatusDot.Fill = new SolidColorBrush(Color.FromArgb(255, 122, 106, 134));
        }
    }
}
