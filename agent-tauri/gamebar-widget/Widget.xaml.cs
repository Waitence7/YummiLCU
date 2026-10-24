using System;
using System.Collections.Generic;
using System.Linq;
using System.IO;
using System.IO.Pipes;
using System.Text;
using System.Threading.Tasks;
using Windows.Data.Json;
using Windows.UI;
using Windows.UI.Xaml;
using Windows.UI.Xaml.Controls;
using Windows.UI.Xaml.Media;
using Windows.UI.Xaml.Media.Animation;

namespace YummiGameBarWidget
{
    public sealed partial class Widget : Page
    {
        private const string PipeName = @"LOCAL\YummiLcuGameBarV1";
        private const string CommandPipeName = @"LOCAL\YummiLcuGameBarCommandV1";
        private readonly DispatcherTimer timer = new DispatcherTimer { Interval = TimeSpan.FromSeconds(1) };
        private bool refreshing;
        private int clickCount;
        private bool detailsOpen;
        private double dragX;
        private double dragY;
        private HashSet<string> previousOnlineFriends = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        private bool onlineFriendsInitialized;
        private readonly DispatcherTimer friendToastTimer = new DispatcherTimer { Interval = TimeSpan.FromSeconds(5) };

        public Widget()
        {
            InitializeComponent();
            timer.Tick += async (_, __) => await RefreshAsync();
            friendToastTimer.Tick += (_, __) => HideFriendOnlineToast();
            Loaded += async (_, __) => { timer.Start(); await RefreshAsync(); };
            Unloaded += (_, __) => { timer.Stop(); friendToastTimer.Stop(); };
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
            catch
            {
                SetOffline("유미 LCU Agent를 실행해 주세요");
            }
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

            var onlineFriends = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
            if (json.ContainsKey("online_friends") && json["online_friends"].ValueType == JsonValueType.Array)
                foreach (var value in json.GetNamedArray("online_friends")) if (value.ValueType == JsonValueType.String) onlineFriends.Add(value.GetString());
            DetectFriendOnlineTransitions(connected, onlineFriends);

            PhaseText.Text = phase;
            RiotIdText.Text = string.IsNullOrWhiteSpace(riotId) ? (connected ? "Riot ID 확인 중" : "League 클라이언트를 기다리는 중") : riotId;
            PartyList.ItemsSource = party;
            EmptyPartyText.Visibility = party.Count == 0 ? Visibility.Visible : Visibility.Collapsed;
            BridgeText.Text = connected ? "LCU 연결됨 · Win+G에서 핀 고정 가능" : "유미 에이전트 연결됨 · League 대기 중";
            StatusDot.Fill = new SolidColorBrush(connected ? Color.FromArgb(255, 86, 221, 139) : Color.FromArgb(255, 122, 106, 134));
        }


        private async void StatusMessageButton_Click(object sender, RoutedEventArgs e)
        {
            var value = (StatusMessageInput.Text ?? string.Empty).Trim();
            if (value.IndexOf('\r') >= 0 || value.IndexOf('\n') >= 0)
            {
                StatusMessageResultText.Text = "줄바꿈은 사용할 수 없습니다.";
                return;
            }

            StatusMessageButton.IsEnabled = false;
            StatusMessageResultText.Text = "LCU에 적용 중…";
            try
            {
                var request = new JsonObject
                {
                    ["command"] = JsonValue.CreateStringValue("set_status_message"),
                    ["value"] = JsonValue.CreateStringValue(value)
                };
                using (var pipe = new NamedPipeClientStream(".", CommandPipeName, PipeDirection.InOut, PipeOptions.Asynchronous))
                {
                    await pipe.ConnectAsync(1000);
                    using (var writer = new StreamWriter(pipe, new UTF8Encoding(false), 1024, true) { AutoFlush = true })
                    using (var reader = new StreamReader(pipe, Encoding.UTF8, false, 4096, true))
                    {
                        await writer.WriteLineAsync(request.Stringify());
                        var line = await reader.ReadLineAsync();
                        if (string.IsNullOrWhiteSpace(line)) throw new IOException("empty command response");
                        var response = JsonObject.Parse(line);
                        var ok = response.GetNamedBoolean("ok", false);
                        var message = response.GetNamedString("message", ok ? "적용 완료" : "적용 실패");
                        StatusMessageResultText.Text = message;
                        InputStatusText.Text = ok ? "LCU 상태 메시지 변경 완료" : $"LCU 변경 실패 · {message}";
                    }
                }
            }
            catch
            {
                StatusMessageResultText.Text = "유미 LCU Agent 명령 채널에 연결하지 못했습니다.";
                InputStatusText.Text = "LCU 상태 메시지 변경 실패";
            }
            finally
            {
                StatusMessageButton.IsEnabled = true;
            }
        }

        private void DetectFriendOnlineTransitions(bool connected, HashSet<string> onlineFriends)
        {
            if (!connected)
            {
                onlineFriendsInitialized = false;
                previousOnlineFriends.Clear();
                return;
            }

            if (!onlineFriendsInitialized)
            {
                previousOnlineFriends = onlineFriends;
                onlineFriendsInitialized = true;
                return;
            }

            var newlyOnline = onlineFriends
                .Where(name => !previousOnlineFriends.Contains(name))
                .OrderBy(name => name, StringComparer.OrdinalIgnoreCase)
                .ToList();
            previousOnlineFriends = onlineFriends;

            if (newlyOnline.Count == 0) return;
            var message = newlyOnline.Count == 1
                ? $"{newlyOnline[0]} 접속함"
                : $"{newlyOnline[0]} 외 {newlyOnline.Count - 1}명 접속함";
            ShowFriendOnlineToast(message);
        }

        private void ShowFriendOnlineToast(string message)
        {
            FriendOnlineToastText.Text = message;
            FriendOnlineToast.Visibility = Visibility.Visible;
            FriendOnlineToast.Opacity = 0;
            FriendToastTranslate.X = 36;

            var fade = new DoubleAnimation
            {
                To = 1,
                Duration = TimeSpan.FromMilliseconds(180),
                EasingFunction = new QuadraticEase { EasingMode = EasingMode.EaseOut }
            };
            var slide = new DoubleAnimation
            {
                To = 0,
                Duration = TimeSpan.FromMilliseconds(220),
                EasingFunction = new QuadraticEase { EasingMode = EasingMode.EaseOut }
            };
            Storyboard.SetTarget(fade, FriendOnlineToast);
            Storyboard.SetTargetProperty(fade, "Opacity");
            Storyboard.SetTarget(slide, FriendToastTranslate);
            Storyboard.SetTargetProperty(slide, "X");
            var storyboard = new Storyboard();
            storyboard.Children.Add(fade);
            storyboard.Children.Add(slide);
            storyboard.Begin();

            friendToastTimer.Stop();
            friendToastTimer.Start();
        }

        private void HideFriendOnlineToast()
        {
            friendToastTimer.Stop();
            if (FriendOnlineToast.Visibility != Visibility.Visible) return;

            var fade = new DoubleAnimation
            {
                To = 0,
                Duration = TimeSpan.FromMilliseconds(180),
                EasingFunction = new QuadraticEase { EasingMode = EasingMode.EaseIn }
            };
            var slide = new DoubleAnimation
            {
                To = 24,
                Duration = TimeSpan.FromMilliseconds(180),
                EasingFunction = new QuadraticEase { EasingMode = EasingMode.EaseIn }
            };
            Storyboard.SetTarget(fade, FriendOnlineToast);
            Storyboard.SetTargetProperty(fade, "Opacity");
            Storyboard.SetTarget(slide, FriendToastTranslate);
            Storyboard.SetTargetProperty(slide, "X");
            var storyboard = new Storyboard();
            storyboard.Children.Add(fade);
            storyboard.Children.Add(slide);
            storyboard.Completed += (_, __) => FriendOnlineToast.Visibility = Visibility.Collapsed;
            storyboard.Begin();
        }

        private void ClickTestButton_Click(object sender, RoutedEventArgs e)
        {
            clickCount++;
            ClickCountText.Text = $"클릭 {clickCount}회";
            InputStatusText.Text = $"버튼 클릭 · {DateTime.Now:HH:mm:ss}";
        }

        private void ResetTests_Click(object sender, RoutedEventArgs e)
        {
            clickCount = 0;
            ClickCountText.Text = "클릭 0회";
            ToggleTest.IsOn = false;
            TestSlider.Value = 50;
            dragX = 0;
            dragY = 0;
            GestureTranslate.X = 0;
            GestureTranslate.Y = 0;
            GestureText.Text = "대기 중";
            InputStatusText.Text = "테스트 초기화";
        }

        private void ToggleTest_Toggled(object sender, RoutedEventArgs e)
        {
            InputStatusText.Text = ToggleTest.IsOn ? "토글 켜짐" : "토글 꺼짐";
        }

        private void TestSlider_ValueChanged(object sender, Windows.UI.Xaml.Controls.Primitives.RangeBaseValueChangedEventArgs e)
        {
            if (SliderValueText == null) return;
            SliderValueText.Text = $"슬라이더 · {Math.Round(e.NewValue)}";
            if (InputStatusText != null) InputStatusText.Text = $"슬라이더 {Math.Round(e.NewValue)}";
        }

        private void GestureCard_ManipulationStarted(object sender, Windows.UI.Xaml.Input.ManipulationStartedRoutedEventArgs e)
        {
            GestureText.Text = "드래그 시작";
            InputStatusText.Text = "드래그 입력 감지";
        }

        private void GestureCard_ManipulationDelta(object sender, Windows.UI.Xaml.Input.ManipulationDeltaRoutedEventArgs e)
        {
            dragX += e.Delta.Translation.X;
            dragY += e.Delta.Translation.Y;
            dragX = Math.Max(-90, Math.Min(90, dragX));
            dragY = Math.Max(-50, Math.Min(50, dragY));
            GestureTranslate.X = dragX;
            GestureTranslate.Y = dragY;
            GestureText.Text = $"x {dragX:0} · y {dragY:0}";
        }

        private void GestureCard_ManipulationCompleted(object sender, Windows.UI.Xaml.Input.ManipulationCompletedRoutedEventArgs e)
        {
            var vx = e.Velocities.Linear.X;
            var vy = e.Velocities.Linear.Y;
            GestureText.Text = $"놓음 · 속도 {vx:0.00}, {vy:0.00}";
            InputStatusText.Text = Math.Abs(dragX) >= 60
                ? (dragX > 0 ? "오른쪽 스와이프 감지" : "왼쪽 스와이프 감지")
                : "드래그 완료";
        }

        private void GestureCard_Tapped(object sender, Windows.UI.Xaml.Input.TappedRoutedEventArgs e)
        {
            InputStatusText.Text = "카드 클릭";
        }

        private void GestureCard_DoubleTapped(object sender, Windows.UI.Xaml.Input.DoubleTappedRoutedEventArgs e)
        {
            InputStatusText.Text = "카드 더블클릭";
        }

        private void GestureCard_RightTapped(object sender, Windows.UI.Xaml.Input.RightTappedRoutedEventArgs e)
        {
            InputStatusText.Text = "카드 우클릭";
        }

        private void DetailsButton_Click(object sender, RoutedEventArgs e)
        {
            detailsOpen = !detailsOpen;
            DetailsPanel.Visibility = detailsOpen ? Visibility.Visible : Visibility.Collapsed;
            DetailsButton.Content = detailsOpen ? "테스트 상세 접기" : "테스트 상세 펼치기";
            InputStatusText.Text = detailsOpen ? "상세 패널 펼침" : "상세 패널 접음";
        }

        private void SetOffline(string message)
        {
            PhaseText.Text = "Offline";
            RiotIdText.Text = "League 클라이언트를 기다리는 중";
            PartyList.ItemsSource = null;
            EmptyPartyText.Visibility = Visibility.Visible;
            BridgeText.Text = message;
            StatusDot.Fill = new SolidColorBrush(Color.FromArgb(255, 122, 106, 134));
        }
    }
}
