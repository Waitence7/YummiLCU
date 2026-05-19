#nullable enable
using YummiLcu.Agent.Ui;

namespace YummiLcu.Agent;

partial class MainForm
{
    private System.ComponentModel.IContainer? components = null;

    private Panel _topBar;
    private Panel _clientArea;
    private Panel _emptyLobby;
    private Panel _drawer;
    private FlowLayoutPanel _emptyActions;
    private FlowLayoutPanel _topConn;
    private FlowLayoutPanel _actionsFlow;
    private FlowLayoutPanel _settingsPanel;
    private FlowLayoutPanel _rowLock;
    private FlowLayoutPanel _rowStatus;
    private GroupBox _grpSettings;
    private GroupBox _grpActions;
    private Label _status;
    private Label _lockfileLabel;
    private Label _statusMsgLabel;
    private TextBox _lockfilePath;
    private TextBox _statusMsg;
    private TextBox _log;
    private CheckBox _testMode;
    private CheckBox _preventDodge;
    private CheckBox _defaultStatus;
    private Button _start;
    private Button _stop;
    private Button _btnSettings;
    private Button _pickFile;
    private Button _pickFolder;
    private Button _applyStatus;
    private Button _btnRankedLobby;
    private Button _btnNormalLobby;
    private Button _btnLeaveLobby;

    protected override void Dispose(bool disposing)
    {
        if (disposing && components is not null)
            components.Dispose();
        base.Dispose(disposing);
    }

    private void InitializeComponent()
    {
        _topBar = new Panel();
        _topConn = new FlowLayoutPanel();
        _start = new Button();
        _stop = new Button();
        _status = new Label();
        _btnSettings = new Button();
        _clientArea = new Panel();
        _emptyLobby = new Panel();
        _emptyActions = new FlowLayoutPanel();
        _btnRankedLobby = new Button();
        _btnNormalLobby = new Button();
        _btnLeaveLobby = new Button();
        _drawer = new Panel();
        _grpSettings = new GroupBox();
        _settingsPanel = new FlowLayoutPanel();
        _testMode = new CheckBox();
        _preventDodge = new CheckBox();
        _defaultStatus = new CheckBox();
        _rowStatus = new FlowLayoutPanel();
        _statusMsgLabel = new Label();
        _statusMsg = new TextBox();
        _applyStatus = new Button();
        _rowLock = new FlowLayoutPanel();
        _lockfileLabel = new Label();
        _lockfilePath = new TextBox();
        _pickFile = new Button();
        _pickFolder = new Button();
        _grpActions = new GroupBox();
        _actionsFlow = new FlowLayoutPanel();
        _log = new TextBox();
        _topBar.SuspendLayout();
        _topConn.SuspendLayout();
        _clientArea.SuspendLayout();
        _emptyLobby.SuspendLayout();
        _emptyActions.SuspendLayout();
        _drawer.SuspendLayout();
        _grpSettings.SuspendLayout();
        _settingsPanel.SuspendLayout();
        _rowStatus.SuspendLayout();
        _rowLock.SuspendLayout();
        _grpActions.SuspendLayout();
        SuspendLayout();
        // 
        // _topBar
        // 
        _topBar.Controls.Add(_topConn);
        _topBar.Controls.Add(_btnSettings);
        _topBar.Dock = DockStyle.Top;
        _topBar.Location = new Point(0, 0);
        _topBar.Name = "_topBar";
        _topBar.Padding = new Padding(12, 8, 12, 8);
        _topBar.Size = new Size(1280, 48);
        _topBar.TabIndex = 0;
        // 
        // _topConn
        // 
        _topConn.AutoSize = true;
        _topConn.Controls.Add(_start);
        _topConn.Controls.Add(_stop);
        _topConn.Controls.Add(_status);
        _topConn.Dock = DockStyle.Fill;
        _topConn.Location = new Point(12, 8);
        _topConn.Name = "_topConn";
        _topConn.Size = new Size(1220, 32);
        _topConn.TabIndex = 0;
        _topConn.WrapContents = false;
        // 
        // _start
        // 
        _start.AutoSize = true;
        _start.FlatStyle = FlatStyle.Flat;
        _start.Location = new Point(3, 3);
        _start.Name = "_start";
        _start.Size = new Size(71, 27);
        _start.TabIndex = 0;
        _start.Text = "연결";
        _start.UseVisualStyleBackColor = true;
        // 
        // _stop
        // 
        _stop.AutoSize = true;
        _stop.Enabled = false;
        _stop.FlatStyle = FlatStyle.Flat;
        _stop.Location = new Point(80, 3);
        _stop.Name = "_stop";
        _stop.Size = new Size(39, 27);
        _stop.TabIndex = 1;
        _stop.Text = "중지";
        _stop.UseVisualStyleBackColor = true;
        // 
        // _status
        // 
        _status.AutoSize = true;
        _status.Location = new Point(125, 8);
        _status.MaximumSize = new Size(900, 0);
        _status.Name = "_status";
        _status.Size = new Size(0, 15);
        _status.TabIndex = 2;
        // 
        // _btnSettings
        // 
        _btnSettings.Anchor = AnchorStyles.Top | AnchorStyles.Right;
        _btnSettings.AutoSize = true;
        _btnSettings.FlatStyle = FlatStyle.Flat;
        _btnSettings.Location = new Point(1232, 10);
        _btnSettings.Name = "_btnSettings";
        _btnSettings.Size = new Size(36, 27);
        _btnSettings.TabIndex = 1;
        _btnSettings.Text = "⚙";
        _btnSettings.UseVisualStyleBackColor = true;
        // 
        // _clientArea
        // 
        _clientArea.Controls.Add(_emptyLobby);
        _clientArea.Dock = DockStyle.Fill;
        _clientArea.Location = new Point(0, 48);
        _clientArea.Name = "_clientArea";
        _clientArea.Size = new Size(1280, 576);
        _clientArea.TabIndex = 1;
        // 
        // _emptyLobby
        // 
        _emptyLobby.Controls.Add(_emptyActions);
        _emptyLobby.Dock = DockStyle.Fill;
        _emptyLobby.Name = "_emptyLobby";
        _emptyLobby.Size = new Size(1280, 576);
        _emptyLobby.TabIndex = 0;
        // 
        // _emptyActions
        // 
        _emptyActions.Anchor = AnchorStyles.None;
        _emptyActions.AutoSize = true;
        _emptyActions.Controls.Add(_btnRankedLobby);
        _emptyActions.Controls.Add(_btnNormalLobby);
        _emptyActions.Controls.Add(_btnLeaveLobby);
        _emptyActions.FlowDirection = FlowDirection.TopDown;
        _emptyActions.Location = new Point(540, 220);
        _emptyActions.Name = "_emptyActions";
        _emptyActions.Size = new Size(200, 132);
        _emptyActions.TabIndex = 0;
        _emptyActions.WrapContents = false;
        // 
        // _btnRankedLobby
        // 
        _btnRankedLobby.AutoSize = true;
        _btnRankedLobby.FlatStyle = FlatStyle.Flat;
        _btnRankedLobby.Location = new Point(3, 3);
        _btnRankedLobby.Name = "_btnRankedLobby";
        _btnRankedLobby.Size = new Size(140, 27);
        _btnRankedLobby.TabIndex = 0;
        _btnRankedLobby.Text = "솔랭 로비 만들기";
        _btnRankedLobby.UseVisualStyleBackColor = true;
        // 
        // _btnNormalLobby
        // 
        _btnNormalLobby.AutoSize = true;
        _btnNormalLobby.FlatStyle = FlatStyle.Flat;
        _btnNormalLobby.Location = new Point(3, 36);
        _btnNormalLobby.Name = "_btnNormalLobby";
        _btnNormalLobby.Size = new Size(140, 27);
        _btnNormalLobby.TabIndex = 1;
        _btnNormalLobby.Text = "일반 로비 만들기";
        _btnNormalLobby.UseVisualStyleBackColor = true;
        // 
        // _btnLeaveLobby
        // 
        _btnLeaveLobby.AutoSize = true;
        _btnLeaveLobby.FlatStyle = FlatStyle.Flat;
        _btnLeaveLobby.Location = new Point(3, 69);
        _btnLeaveLobby.Name = "_btnLeaveLobby";
        _btnLeaveLobby.Size = new Size(90, 27);
        _btnLeaveLobby.TabIndex = 2;
        _btnLeaveLobby.Text = "로비 나가기";
        _btnLeaveLobby.UseVisualStyleBackColor = true;
        _btnLeaveLobby.Visible = false;
        // 
        // _drawer
        // 
        _drawer.Controls.Add(_log);
        _drawer.Controls.Add(_grpActions);
        _drawer.Controls.Add(_grpSettings);
        _drawer.Dock = DockStyle.Right;
        _drawer.Location = new Point(920, 48);
        _drawer.Name = "_drawer";
        _drawer.Padding = new Padding(8);
        _drawer.Size = new Size(360, 672);
        _drawer.TabIndex = 2;
        _drawer.Visible = false;
        // 
        // _grpSettings
        // 
        _grpSettings.Controls.Add(_settingsPanel);
        _grpSettings.Controls.Add(_rowLock);
        _grpSettings.Dock = DockStyle.Top;
        _grpSettings.Location = new Point(8, 8);
        _grpSettings.Name = "_grpSettings";
        _grpSettings.Padding = new Padding(8);
        _grpSettings.Size = new Size(344, 242);
        _grpSettings.TabIndex = 0;
        _grpSettings.TabStop = false;
        _grpSettings.Text = "설정";
        // 
        // _settingsPanel
        // 
        _settingsPanel.AutoSize = true;
        _settingsPanel.Controls.Add(_testMode);
        _settingsPanel.Controls.Add(_preventDodge);
        _settingsPanel.Controls.Add(_defaultStatus);
        _settingsPanel.Controls.Add(_rowStatus);
        _settingsPanel.Dock = DockStyle.Top;
        _settingsPanel.FlowDirection = FlowDirection.TopDown;
        _settingsPanel.Location = new Point(8, 24);
        _settingsPanel.Name = "_settingsPanel";
        _settingsPanel.Size = new Size(328, 120);
        _settingsPanel.TabIndex = 0;
        _settingsPanel.WrapContents = false;
        // 
        // _testMode
        // 
        _testMode.AutoSize = true;
        _testMode.Location = new Point(3, 3);
        _testMode.Name = "_testMode";
        _testMode.Size = new Size(220, 19);
        _testMode.TabIndex = 0;
        _testMode.Text = "테스트 모드 (롤 접속 없이 UI)";
        _testMode.UseVisualStyleBackColor = true;
        // 
        // _preventDodge
        // 
        _preventDodge.AutoSize = true;
        _preventDodge.Checked = true;
        _preventDodge.CheckState = CheckState.Checked;
        _preventDodge.Location = new Point(3, 28);
        _preventDodge.Name = "_preventDodge";
        _preventDodge.Size = new Size(198, 19);
        _preventDodge.TabIndex = 0;
        _preventDodge.Text = "닷지 후 매칭 자동 재시작 방지";
        _preventDodge.UseVisualStyleBackColor = true;
        // 
        // _defaultStatus
        // 
        _defaultStatus.AutoSize = true;
        _defaultStatus.Checked = true;
        _defaultStatus.CheckState = CheckState.Checked;
        _defaultStatus.Location = new Point(3, 53);
        _defaultStatus.Name = "_defaultStatus";
        _defaultStatus.Size = new Size(150, 19);
        _defaultStatus.TabIndex = 1;
        _defaultStatus.Text = "연결 시 기본 상메 적용";
        _defaultStatus.UseVisualStyleBackColor = true;
        // 
        // _rowStatus
        // 
        _rowStatus.AutoSize = true;
        _rowStatus.Controls.Add(_statusMsgLabel);
        _rowStatus.Controls.Add(_statusMsg);
        _rowStatus.Controls.Add(_applyStatus);
        _rowStatus.Location = new Point(3, 78);
        _rowStatus.Name = "_rowStatus";
        _rowStatus.Size = new Size(420, 54);
        _rowStatus.TabIndex = 2;
        _rowStatus.WrapContents = true;
        // 
        // _statusMsgLabel
        // 
        _statusMsgLabel.AutoSize = true;
        _statusMsgLabel.Location = new Point(3, 3);
        _statusMsgLabel.Name = "_statusMsgLabel";
        _statusMsgLabel.Size = new Size(31, 15);
        _statusMsgLabel.TabIndex = 0;
        _statusMsgLabel.Text = "상메:";
        // 
        // _statusMsg
        // 
        _statusMsg.Location = new Point(40, 3);
        _statusMsg.Multiline = true;
        _statusMsg.Name = "_statusMsg";
        _statusMsg.ScrollBars = ScrollBars.Vertical;
        _statusMsg.Size = new Size(240, 48);
        _statusMsg.TabIndex = 1;
        // 
        // _applyStatus
        // 
        _applyStatus.AutoSize = true;
        _applyStatus.Location = new Point(286, 3);
        _applyStatus.Name = "_applyStatus";
        _applyStatus.Size = new Size(71, 23);
        _applyStatus.TabIndex = 2;
        _applyStatus.Text = "상메 적용";
        _applyStatus.UseVisualStyleBackColor = true;
        // 
        // _rowLock
        // 
        _rowLock.AutoSize = true;
        _rowLock.Controls.Add(_lockfileLabel);
        _rowLock.Controls.Add(_lockfilePath);
        _rowLock.Controls.Add(_pickFile);
        _rowLock.Controls.Add(_pickFolder);
        _rowLock.Dock = DockStyle.Top;
        _rowLock.Location = new Point(8, 122);
        _rowLock.Name = "_rowLock";
        _rowLock.Size = new Size(328, 29);
        _rowLock.TabIndex = 1;
        _rowLock.WrapContents = true;
        // 
        // _lockfileLabel
        // 
        _lockfileLabel.AutoSize = true;
        _lockfileLabel.Location = new Point(3, 6);
        _lockfileLabel.Name = "_lockfileLabel";
        _lockfileLabel.Size = new Size(52, 15);
        _lockfileLabel.TabIndex = 0;
        _lockfileLabel.Text = "lockfile:";
        // 
        // _lockfilePath
        // 
        _lockfilePath.Location = new Point(61, 3);
        _lockfilePath.Name = "_lockfilePath";
        _lockfilePath.PlaceholderText = "lockfile 경로";
        _lockfilePath.Size = new Size(180, 23);
        _lockfilePath.TabIndex = 1;
        // 
        // _pickFile
        // 
        _pickFile.AutoSize = true;
        _pickFile.Location = new Point(247, 3);
        _pickFile.Name = "_pickFile";
        _pickFile.Size = new Size(39, 23);
        _pickFile.TabIndex = 2;
        _pickFile.Text = "파일";
        _pickFile.UseVisualStyleBackColor = true;
        // 
        // _pickFolder
        // 
        _pickFolder.AutoSize = true;
        _pickFolder.Location = new Point(3, 32);
        _pickFolder.Name = "_pickFolder";
        _pickFolder.Size = new Size(59, 23);
        _pickFolder.TabIndex = 3;
        _pickFolder.Text = "롤 폴더";
        _pickFolder.UseVisualStyleBackColor = true;
        // 
        // _grpActions
        // 
        _grpActions.Controls.Add(_actionsFlow);
        _grpActions.Dock = DockStyle.Top;
        _grpActions.Location = new Point(8, 228);
        _grpActions.Name = "_grpActions";
        _grpActions.Padding = new Padding(8);
        _grpActions.Size = new Size(344, 120);
        _grpActions.TabIndex = 1;
        _grpActions.TabStop = false;
        _grpActions.Text = "LCU 명령";
        // 
        // _actionsFlow
        // 
        _actionsFlow.AutoScroll = true;
        _actionsFlow.Dock = DockStyle.Fill;
        _actionsFlow.Location = new Point(8, 24);
        _actionsFlow.Name = "_actionsFlow";
        _actionsFlow.Size = new Size(328, 88);
        _actionsFlow.TabIndex = 0;
        _actionsFlow.WrapContents = true;
        // 
        // _log
        // 
        _log.Dock = DockStyle.Fill;
        _log.Location = new Point(8, 348);
        _log.Multiline = true;
        _log.Name = "_log";
        _log.ReadOnly = true;
        _log.ScrollBars = ScrollBars.Vertical;
        _log.Size = new Size(344, 316);
        _log.TabIndex = 2;
        // 
        // MainForm
        // 
        AutoScaleDimensions = new SizeF(7F, 15F);
        AutoScaleMode = AutoScaleMode.Font;
        ClientSize = new Size(1280, 720);
        Controls.Add(_clientArea);
        Controls.Add(_drawer);
        Controls.Add(_topBar);
        MinimumSize = new Size(1024, 640);
        Name = "MainForm";
        StartPosition = FormStartPosition.CenterScreen;
        Text = "YummiLcu Agent";
        _topBar.ResumeLayout(false);
        _topBar.PerformLayout();
        _topConn.ResumeLayout(false);
        _topConn.PerformLayout();
        _clientArea.ResumeLayout(false);
        _emptyLobby.ResumeLayout(false);
        _emptyActions.ResumeLayout(false);
        _emptyActions.PerformLayout();
        _drawer.ResumeLayout(false);
        _drawer.PerformLayout();
        _grpSettings.ResumeLayout(false);
        _grpSettings.PerformLayout();
        _settingsPanel.ResumeLayout(false);
        _settingsPanel.PerformLayout();
        _rowStatus.ResumeLayout(false);
        _rowStatus.PerformLayout();
        _rowLock.ResumeLayout(false);
        _rowLock.PerformLayout();
        _grpActions.ResumeLayout(false);
        ResumeLayout(false);
    }
}
