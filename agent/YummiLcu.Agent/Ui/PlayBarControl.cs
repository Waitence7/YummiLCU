namespace YummiLcu.Agent.Ui;

/// <summary>하단 게임 찾기 바 — 매칭 시 예상 시간 + 간단한 금색 펄스.</summary>
internal sealed class PlayBarControl : Panel
{
    private readonly Label _eta = new()
    {
        AutoSize = true,
        ForeColor = ClientTheme.Searching,
        Font = ClientTheme.BodyFont,
        TextAlign = ContentAlignment.MiddleCenter,
        Visible = false,
    };
    private readonly Label _elapsed = new()
    {
        AutoSize = true,
        ForeColor = ClientTheme.TextMuted,
        Font = ClientTheme.SmallFont,
        TextAlign = ContentAlignment.MiddleCenter,
        Visible = false,
    };
    private readonly Panel _playButton = new() { Cursor = Cursors.Hand };
    private readonly Label _playLabel = new()
    {
        AutoSize = false,
        Dock = DockStyle.Fill,
        TextAlign = ContentAlignment.MiddleCenter,
        Font = ClientTheme.PlayFont,
        ForeColor = Color.FromArgb(40, 32, 16),
        Text = "게임 찾기",
    };

    private readonly System.Windows.Forms.Timer _pulse = new() { Interval = 48 };
    private float _pulsePhase;
    private bool _searching;
    private bool _lobbyReady;
    private bool _hover;

    public event EventHandler? PlayClicked;

    public PlayBarControl()
    {
        Height = 96;
        Dock = DockStyle.Bottom;
        BackColor = ClientTheme.BgPanel;
        DoubleBuffered = true;
        Padding = new Padding(0, 6, 0, 10);

        _playButton.Size = new Size(280, 44);
        _playButton.Controls.Add(_playLabel);
        _playButton.Paint += PlayButton_Paint;
        _playButton.MouseEnter += (_, _) => { _hover = true; _playButton.Invalidate(); };
        _playButton.MouseLeave += (_, _) => { _hover = false; _playButton.Invalidate(); };
        _playButton.Click += (_, _) => PlayClicked?.Invoke(this, EventArgs.Empty);

        var stack = new FlowLayoutPanel
        {
            AutoSize = true,
            FlowDirection = FlowDirection.TopDown,
            WrapContents = false,
            Anchor = AnchorStyles.None,
        };
        stack.Controls.Add(_playButton);
        stack.Controls.Add(_eta);
        stack.Controls.Add(_elapsed);
        Controls.Add(stack);
        Resize += (_, _) => CenterStack(stack);

        _pulse.Tick += (_, _) =>
        {
            if (!_searching)
                return;
            _pulsePhase += 0.12f;
            Invalidate();
            _playButton.Invalidate();
        };
    }

    public void SetLobbyReady(bool ready)
    {
        _lobbyReady = ready;
        if (!ready && !_searching)
            _playLabel.Text = "로비를 만드세요";
        else if (!_searching)
            _playLabel.Text = "게임 찾기";
        _playButton.Enabled = ready || _searching;
        _playButton.Invalidate();
    }

    public void ApplyMatchmaking(MatchmakingStatus status)
    {
        _searching = status.IsSearching;
        if (_searching)
        {
            _pulse.Start();
            _playLabel.Text = "찾는 중…";
            _playLabel.ForeColor = ClientTheme.Text;
            _eta.Visible = true;
            _elapsed.Visible = true;
            _eta.Text = $"예상 대기  {MatchmakingStatus.FormatDuration(status.EstimatedQueueTimeSeconds)}";
            _elapsed.Text = $"경과  {MatchmakingStatus.FormatDuration(status.TimeInQueueSeconds)}";
        }
        else
        {
            _pulse.Stop();
            _pulsePhase = 0;
            _playLabel.ForeColor = Color.FromArgb(40, 32, 16);
            _eta.Visible = false;
            _elapsed.Visible = false;
            _eta.Text = "";
            _elapsed.Text = "";
            if (_lobbyReady)
                _playLabel.Text = "게임 찾기";
            else
                _playLabel.Text = "로비를 만드세요";
        }
        _playButton.Invalidate();
        Invalidate();
    }

    protected override void OnPaint(PaintEventArgs e)
    {
        base.OnPaint(e);
        if (!_searching)
            return;

        var g = e.Graphics;
        var alpha = (int)(18 + 14 * Math.Sin(_pulsePhase));
        using var line = new Pen(Color.FromArgb(alpha, ClientTheme.Gold), 2);
        var y = Height - 3;
        g.DrawLine(line, 40, y, Width - 40, y);
    }

    private void PlayButton_Paint(object? sender, PaintEventArgs e)
    {
        var g = e.Graphics;
        g.SmoothingMode = System.Drawing.Drawing2D.SmoothingMode.AntiAlias;
        var r = _playButton.ClientRectangle;
        r.Inflate(-1, -1);

        var top = _hover ? ClientTheme.GoldBright : ClientTheme.Gold;
        var bottom = _searching
            ? Color.FromArgb(140, 115, 60)
            : Color.FromArgb(_lobbyReady ? 160 : 100, _lobbyReady ? 130 : 80, _lobbyReady ? 70 : 45);

        using var brush = new System.Drawing.Drawing2D.LinearGradientBrush(r, top, bottom, 90f);
        g.FillRectangle(brush, r);

        if (_searching)
        {
            var glow = (int)(40 + 35 * Math.Sin(_pulsePhase));
            using var glowPen = new Pen(Color.FromArgb(glow, ClientTheme.GoldBright), 2);
            g.DrawRectangle(glowPen, r.X + 1, r.Y + 1, r.Width - 3, r.Height - 3);
        }
        else
        {
            using var border = new Pen(Color.FromArgb(80, 60, 30), 1);
            g.DrawRectangle(border, r.X, r.Y, r.Width - 1, r.Height - 1);
        }
    }

    private void CenterStack(FlowLayoutPanel stack)
    {
        stack.PerformLayout();
        stack.Location = new Point(
            Math.Max(0, (ClientSize.Width - stack.Width) / 2),
            Math.Max(0, (ClientSize.Height - stack.Height) / 2));
    }
}
