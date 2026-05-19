namespace YummiLcu.Agent.Ui;

internal sealed class LobbyClientPanel : Panel
{
    private readonly Label _queueTitle = new()
    {
        AutoSize = false,
        TextAlign = ContentAlignment.MiddleCenter,
        Dock = DockStyle.Top,
        Height = 44,
        Font = ClientTheme.TitleFont,
        ForeColor = ClientTheme.Text,
    };
    private readonly FlowLayoutPanel _slots = new()
    {
        AutoSize = true,
        WrapContents = false,
        FlowDirection = FlowDirection.LeftToRight,
        Anchor = AnchorStyles.None,
    };
    private readonly Label _memberHint = new()
    {
        AutoSize = true,
        ForeColor = ClientTheme.TextMuted,
        Font = ClientTheme.SmallFont,
        TextAlign = ContentAlignment.MiddleCenter,
    };

    public LobbyClientPanel()
    {
        BackColor = Color.Transparent;
        DoubleBuffered = true;
        var card = new Panel
        {
            BackColor = ClientTheme.BgPanel,
            Size = new Size(520, 200),
            Padding = new Padding(16),
        };
        card.Paint += (_, e) =>
        {
            var g = e.Graphics;
            using var pen = new Pen(ClientTheme.Border, 1);
            g.DrawRectangle(pen, 0, 0, card.Width - 1, card.Height - 1);
        };

        var inner = new TableLayoutPanel
        {
            Dock = DockStyle.Fill,
            RowCount = 3,
            ColumnCount = 1,
        };
        inner.RowStyles.Add(new RowStyle(SizeType.AutoSize));
        inner.RowStyles.Add(new RowStyle(SizeType.Percent, 100));
        inner.RowStyles.Add(new RowStyle(SizeType.AutoSize));

        _queueTitle.Text = "로비";
        inner.Controls.Add(_queueTitle, 0, 0);

        var slotHost = new Panel { Dock = DockStyle.Fill, Height = 100 };
        slotHost.Controls.Add(_slots);
        slotHost.Resize += (_, _) => CenterSlots();
        inner.Controls.Add(slotHost, 0, 1);

        var hintRow = new FlowLayoutPanel
        {
            AutoSize = true,
            FlowDirection = FlowDirection.LeftToRight,
            WrapContents = false,
            Anchor = AnchorStyles.None,
        };
        hintRow.Controls.Add(_memberHint);
        inner.Controls.Add(hintRow, 0, 2);

        card.Controls.Add(inner);
        Controls.Add(card);
        Resize += (_, _) => CenterCard(card);
        CenterCard(card);
    }

    public void Apply(LobbyInfo lobby)
    {
        _queueTitle.Text = lobby.QueueLabel;
        _memberHint.Text = $"{lobby.MemberCount} / {lobby.MaxMembers}";
        RebuildSlots(lobby.MaxMembers, lobby.MemberCount);
    }

    private void RebuildSlots(int max, int filled)
    {
        _slots.Controls.Clear();
        for (var i = 0; i < max; i++)
            _slots.Controls.Add(new LobbySlotControl(i < filled));
        CenterSlots();
    }

    private void CenterSlots()
    {
        if (_slots.Parent is not Panel host)
            return;
        _slots.PerformLayout();
        _slots.Location = new Point(
            Math.Max(0, (host.ClientSize.Width - _slots.Width) / 2),
            Math.Max(0, (host.ClientSize.Height - _slots.Height) / 2));
    }

    private void CenterCard(Control card)
    {
        card.Location = new Point(
            Math.Max(0, (ClientSize.Width - card.Width) / 2),
            Math.Max(0, (ClientSize.Height - card.Height) / 2));
    }
}

internal sealed class LobbySlotControl : Control
{
    public LobbySlotControl(bool occupied)
    {
        Size = new Size(72, 88);
        DoubleBuffered = true;
        Tag = occupied;
    }

    protected override void OnPaint(PaintEventArgs e)
    {
        var g = e.Graphics;
        g.SmoothingMode = System.Drawing.Drawing2D.SmoothingMode.AntiAlias;
        var occupied = Tag is true;
        var fill = occupied ? ClientTheme.BgSlot : Color.FromArgb(28, 34, 46);
        using var brush = new SolidBrush(fill);
        g.FillRectangle(brush, 1, 1, Width - 2, Height - 2);
        using var pen = new Pen(occupied ? ClientTheme.Accent : ClientTheme.Border, occupied ? 2 : 1);
        g.DrawRectangle(pen, 1, 1, Width - 3, Height - 3);
        if (!occupied)
        {
            using var plusPen = new Pen(ClientTheme.TextMuted, 2);
            var cx = Width / 2;
            var cy = Height / 2;
            g.DrawLine(plusPen, cx - 10, cy, cx + 10, cy);
            g.DrawLine(plusPen, cx, cy - 10, cx, cy + 10);
        }
    }
}
