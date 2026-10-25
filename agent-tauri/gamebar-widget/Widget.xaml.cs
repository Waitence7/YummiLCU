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
                widget.MaxWindowSize = new Windows.Foundation.Size(width, height);
                await widget.TryResizeWindowAsync(new Windows.Foundation.Size(width, height));
                await widget.CenterWindowAsync();
            }
            catch
            {
                // Experimental: keep manifest size if Game Bar rejects full-display resize.
            }
        }

        private async Task RefreshAsync()
        {
            if (refreshing) return;
            refreshing = true;
            try
            {
                using (var pipe = new NamedPipeClientStream(".", PipeName, PipeDirection.In, PipeOptions.Asynchronous))
                {
                    await pipe.ConnectAsync(650);
                    using (var reader = new StreamReader(pipe, Encoding.UTF8, false, 4096, false))
                    {
                        var line = await reader.ReadLineAsync();
                        if (string.IsNullOrWhiteSpace(line)) throw new IOException("empty bridge response");
                        Apply(JsonObject.Parse(line));
                    }
                }
            }
            catch { SetOffline(); }
            finally { refreshing = false; }
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
            BridgeText.Text = connected ? "LCU 연결됨 · 전체 화면 HUD 실험" : "유미 에이전트 연결됨 · League 대기 중";
            StatusDot.Fill = new SolidColorBrush(connected ? Color.FromArgb(255, 86, 221, 139) : Color.FromArgb(255, 122, 106, 134));
        }

        private void SetOffline()
        {
            PhaseText.Text = "Offline";
            RiotIdText.Text = "유미 LCU Agent를 실행해 주세요";
            PartyText.Text = "파티 정보 없음";
            BridgeText.Text = "에이전트 연결 안 됨";
            StatusDot.Fill = new SolidColorBrush(Color.FromArgb(255, 122, 106, 134));
        }
    }
}
