#include "mainwindow.h"
#include "presentation.h"
#include <QApplication>
#include <QCheckBox>
#include <QDesktopServices>
#include <QComboBox>
#include <QDialogButtonBox>
#include <QFileDialog>
#include <QFormLayout>
#include <QGridLayout>
#include <QKeyEvent>
#include <QLineEdit>
#include <QMenu>
#include <QMessageBox>
#include <QPainter>
#include <QScreen>
#include <QResizeEvent>
#include <QStyleOptionButton>
#include <QToolButton>
#include <QUrl>

namespace {
QLabel *label(const QString &text, QWidget *parent = nullptr) {
    auto *result = new QLabel(text, parent);
    result->setTextFormat(Qt::PlainText);
    return result;
}
QString str(const QJsonObject &obj, const char *key) {
    return obj[key].toString();
}
QString errorText(const QJsonObject &reply) {
    return reply["error"].toObject()["message"].toString();
}
QJsonObject gameOf(const QJsonObject &state, const QString &id) {
    return state["games"].toObject()[id].toObject();
}

class HeadingButton : public QPushButton {
  public:
    using QPushButton::QPushButton;
    QSize minimumSizeHint() const override { return {0, sizeHint().height()}; }

  protected:
    void keyPressEvent(QKeyEvent *event) override {
        if (event->key() == Qt::Key_Return || event->key() == Qt::Key_Enter) {
            click();
            event->accept();
        } else {
            QPushButton::keyPressEvent(event);
        }
    }
    void paintEvent(QPaintEvent *) override {
        QStyleOptionButton option;
        initStyleOption(&option);
        option.text = fontMetrics().elidedText(text(), Qt::ElideRight, width());
        QPainter painter(this);
        style()->drawControl(QStyle::CE_PushButton, &option, &painter, this);
    }
};
class Footer : public QWidget {
  public:
    explicit Footer(bool demo, QWidget *parent) : QWidget(parent) {
        setObjectName("footer");
        grid_ = new QGridLayout(this);
        grid_->setContentsMargins(10, 8, 10, 8);
        grid_->setHorizontalSpacing(12);
        hints_ = new QWidget;
        auto *hints = new QHBoxLayout(hints_);
        hints->setContentsMargins(0, 0, 0, 0);
        hints->setSpacing(5);
        for (auto text : {"Save", "Ctrl + F5", "Load", "Ctrl + F9"}) {
            auto *item = label(text);
            if (QString(text).startsWith("Ctrl"))
                item->setObjectName("keycap");
            hints->addWidget(item);
        }
        hints_->setToolTip("Global shortcuts target the top running game, including while this window is closed.");
        sounds_ = new QCheckBox("Play sounds");
        startup_ = new QCheckBox("Launch on startup");
        for (auto *check : {sounds_, startup_}) {
            check->setChecked(demo);
            check->setEnabled(demo);
            check->setToolTip(
                demo ? "Demo setting; no system changes are made."
                     : "Not available yet: requires background-host platform integration.");
        }
        sounds_->setObjectName("playSounds");
        sounds_->setChecked(true);
        sounds_->setEnabled(false);
        sounds_->setToolTip("Play sound cues for operations.");
        startup_->setToolTip("Start the background host in the tray when you sign in.");
        startup_->setObjectName("launchOnStartup");
        arrange();
    }
    QCheckBox *soundControl() const { return sounds_; }
    QCheckBox *startupControl() const { return startup_; }

  protected:
    void resizeEvent(QResizeEvent *) override { arrange(); }

  private:
    void arrange() {
        grid_->removeWidget(hints_);
        grid_->removeWidget(sounds_);
        grid_->removeWidget(startup_);
        for (int col = 0; col < 4; ++col)
            grid_->setColumnStretch(col, 0);
        const int needed = hints_->sizeHint().width() + sounds_->sizeHint().width() +
                           startup_->sizeHint().width() + 65;
        grid_->addWidget(hints_, 0, 0);
        if (width() >= needed) {
            grid_->addWidget(sounds_, 0, 1);
            grid_->setColumnStretch(2, 1);
            grid_->addWidget(startup_, 0, 3, Qt::AlignRight);
        } else {
            grid_->removeWidget(hints_);
            grid_->addWidget(hints_, 0, 0, 1, 3, Qt::AlignLeft);
            grid_->addWidget(sounds_, 1, 0);
            grid_->setColumnStretch(1, 1);
            grid_->addWidget(startup_, 1, 2, Qt::AlignRight);
        }
    }
    QGridLayout *grid_;
    QWidget *hints_;
    QCheckBox *sounds_, *startup_;
};
} // namespace

void LoadButton::setAge(const QString &age, const QString &full) {
    age_ = age;
    setToolTip(full);
    setAccessibleName(age.isEmpty() ? "Load" : "Load, " + full);
    updateGeometry();
    update();
}
QSize LoadButton::sizeHint() const {
    auto smaller = font();
    smaller.setPointSizeF(qMax(8.0, font().pointSizeF() - 1));
    return {
        qMax(QPushButton::sizeHint().width(), QFontMetrics(smaller).horizontalAdvance(age_) + 28),
        qMax(36,
             fontMetrics().height() + (age_.isEmpty() ? 12 : QFontMetrics(smaller).height() + 4))};
}
void LoadButton::paintEvent(QPaintEvent *) {
    QStyleOptionButton option;
    initStyleOption(&option);
    option.text.clear();
    QPainter painter(this);
    style()->drawControl(QStyle::CE_PushButton, &option, &painter, this);
    painter.setPen(
        palette().color(isEnabled() ? QPalette::Active : QPalette::Disabled, QPalette::ButtonText));
    if (age_.isEmpty()) {
        painter.drawText(rect(), Qt::AlignCenter, "Load");
        return;
    }
    painter.drawText(QRect(4, 1, width() - 8, height() / 2), Qt::AlignCenter, "Load");
    auto smaller = font();
    smaller.setPointSizeF(qMax(8.0, font().pointSizeF() - 1));
    painter.setFont(smaller);
    painter.setPen(palette().color(isEnabled() ? QPalette::Active : QPalette::Disabled,
                                   QPalette::PlaceholderText));
    painter.drawText(QRect(6, height() / 2 - 1, width() - 12, height() / 2), Qt::AlignCenter,
                     painter.fontMetrics().elidedText(age_, Qt::ElideRight, qMax(0, width() - 12)));
}
GameRow::GameRow(const QString &gameId, QWidget *parent) : QWidget(parent), id(gameId) {
    setObjectName("gameRow");
    setAttribute(Qt::WA_StyledBackground);
    setSizePolicy(QSizePolicy::Expanding, QSizePolicy::Maximum);
    auto *layout = new QVBoxLayout(this);
    layout->setContentsMargins(18, 12, 18, 12);
    layout->setSpacing(8);
    auto *heading = new QHBoxLayout;
    heading->setSpacing(8);
    icon_ = label("");
    icon_->setObjectName("gameIcon");
    icon_->setFixedSize(27, 27);
    icon_->setAlignment(Qt::AlignCenter);
    heading->addWidget(icon_);
    header = new HeadingButton;
    header->setObjectName("gameTitle");
    header->setCheckable(true);
    header->setSizePolicy(QSizePolicy::Maximum, QSizePolicy::Preferred);
    heading->addWidget(header);
    status_ = label("");
    status_->setObjectName("gameStatus");
    heading->addWidget(status_);
    heading->addStretch();
    layout->addLayout(heading);
    details_ = new QWidget;
    details_->setObjectName("details");
    auto *detailsLayout = new QVBoxLayout(details_);
    detailsLayout->setContentsMargins(0, 0, 0, 0);
    detailsLayout->setSpacing(10);
    controls_ = new QWidget;
    controls_->setObjectName("controls");
    pair_ = new QWidget(controls_);
    save = new QPushButton("Save", pair_);
    save->setObjectName("save");
    // Small neutral line icon, using Qt's SVG renderer through QIcon.
    save->setIcon(QIcon(":/save.svg"));
    split_ = new QWidget(pair_);
    load = new LoadButton("Load", split_);
    load->setObjectName("load");
    arrow = new QPushButton(split_);
    arrow->setIcon(QIcon(":/chevron.svg"));
    arrow->setObjectName("historyArrow");
    arrow->setAccessibleName("Load history");
    arrow->setToolTip("Load history");
    more = new QPushButton(controls_);
    more->setIcon(QIcon(":/ellipsis.svg"));
    more->setObjectName("more");
    more->setAccessibleName("Game options");
    progress = new QProgressBar(pair_);
    progress->setObjectName("operationProgress");
    progress->setRange(0, 0);
    progress->setTextVisible(false);
    progress->hide();
    detailsLayout->addWidget(controls_);
    info = new QLabel;
    info->setObjectName("instructions");
    info->setWordWrap(true);
    info->setTextFormat(Qt::RichText);
    info->setTextInteractionFlags(Qt::TextSelectableByMouse | Qt::TextSelectableByKeyboard);
    info->setSizePolicy(QSizePolicy::Ignored, QSizePolicy::Preferred);
    detailsLayout->addWidget(info);
    error = label("");
    error->setObjectName("error");
    error->setWordWrap(true);
    error->hide();
    detailsLayout->addWidget(error);
    recovery = new QPushButton("Recovery needed…");
    recovery->setObjectName("recovery");
    recovery->hide();
    detailsLayout->addWidget(recovery, 0, Qt::AlignLeft);
    layout->addWidget(details_);
    for (auto *widget : findChildren<QWidget *>())
        widget->installEventFilter(this);
    connect(header, &QPushButton::clicked, this, [this] {
        header->setChecked(true);
        emit selected();
    });
    connect(save, &QPushButton::clicked, this, [this] {
        emit selected();
        emit action({{"type", "save"}});
    });
    connect(load, &QPushButton::clicked, this, [this] {
        emit selected();
        emit action({{"type", "load"}, {"target", QJsonValue::Null}});
    });
    connect(arrow, &QPushButton::clicked, this, [this] {
        emit selected();
        emit showHistory();
    });
    connect(more, &QPushButton::clicked, this, [this] {
        emit selected();
        emit showOptions();
    });
    connect(recovery, &QPushButton::clicked, this, &GameRow::showRecovery);
}
bool GameRow::eventFilter(QObject *, QEvent *event) {
    if (event->type() == QEvent::MouseButtonPress &&
        static_cast<QMouseEvent *>(event)->button() == Qt::LeftButton)
        emit selected();
    return false;
}
void GameRow::mousePressEvent(QMouseEvent *event) {
    if (event->button() == Qt::LeftButton)
        emit selected();
    QWidget::mousePressEvent(event);
}
void GameRow::resizeEvent(QResizeEvent *) {
    layout()->setContentsMargins(width() < 500 ? 14 : 18, width() < 500 ? 10 : 12,
                                 width() < 500 ? 14 : 18, width() < 500 ? 10 : 12);
    arrangeControls();
}
void GameRow::arrangeControls() {
    const int arrowWidth = qMax(28, arrow->fontMetrics().height() + 12);
    const int height = qMax(save->sizeHint().height(), load->sizeHint().height());
    const int preferred = qMax(save->sizeHint().width(), load->sizeHint().width() + arrowWidth);
    const int inset = width() < 500 ? 28 : 36;
    const int available = qMax(40, width() - inset - 40);
    const int equalWidth = qMax(20, qMin(preferred, (available - 6) / 2));
    controls_->setFixedHeight(height);
    pair_->setGeometry(0, 0, equalWidth * 2 + 6, height);
    save->setGeometry(0, 0, equalWidth, height);
    split_->setGeometry(equalWidth + 6, 0, equalWidth, height);
    load->setGeometry(0, 0, qMax(1, equalWidth - arrowWidth), height);
    arrow->setGeometry(qMax(1, equalWidth - arrowWidth), 0, arrowWidth, height);
    more->setGeometry(pair_->width() + 6, 0, 28, height);
    progress->setGeometry(0, height - 2, pair_->width(), 2);
    progress->raise();
}
void GameRow::updateState(const QJsonObject &state, bool selected, bool connected,
                          bool submitting) {
    const auto game = gameOf(state, id);
    const auto name = str(game, "name");
    header->setText(name);
    header->setToolTip(name);
    header->setChecked(selected);
    header->setAccessibleName(name);
    header->setAccessibleDescription(selected ? "Selected game" : "Select game");
    const auto iconPath = state["artwork"].toObject()[id].toObject()["icon_path"].toString();
    const auto artworkRevision = state["artwork_revision"].toInteger();
    if (property("iconGameName").toString() != name ||
        property("iconPath").toString() != iconPath ||
        property("artworkRevision").toLongLong() != artworkRevision) {
        setProperty("iconGameName", name);
        setProperty("iconPath", iconPath);
        setProperty("artworkRevision", artworkRevision);
        QString initials;
        for (const auto &word : name.split(' ', Qt::SkipEmptyParts))
            if (initials.size() < 3)
                initials += word.front();
        if (name.contains(':'))
            initials = name.section(':', 0, 0);
        icon_->setText(initials.toUpper());
        if (!iconPath.isEmpty()) {
            const QPixmap icon(iconPath);
            if (!icon.isNull())
                icon_->setPixmap(icon.scaled(27, 27, Qt::KeepAspectRatio, Qt::SmoothTransformation));
        }
    }
    const bool running = state["active_stack"].toArray().contains(id);
    const auto availability = state["availability"].toObject()[id].toObject();
    const bool dataAvailable = availability["data_available"].toBool();
    const auto checkpoint =
        state["snapshots"].toObject()[availability["default_snapshot_id"].toString()].toObject();
    const auto rows = Presentation::history(state, id);
    const auto op = Presentation::blockingOperation(state, id);
    const bool busy = submitting || op["status"] == "pending";
    const bool blocked =
        !connected || busy || !op.isEmpty() || game["configuration_error"].isString();
    status_->setText(op["status"] == "recovery_needed"                          ? "Recovery needed"
                     : running                                                  ? "Running"
                     : !dataAvailable && rows.isEmpty() && checkpoint.isEmpty() ? "Not run yet"
                                                                                : "");
    status_->setProperty("running", running);
    status_->style()->unpolish(status_);
    status_->style()->polish(status_);
    save->setEnabled(!blocked && dataAvailable);
    load->setEnabled(!blocked && dataAvailable && !checkpoint.isEmpty());
    arrow->setEnabled(connected && !busy && !rows.isEmpty());
    more->setEnabled(true);
    QString age, full;
    if (!checkpoint.isEmpty()) {
        if (checkpoint["saved_at"].isDouble()) {
            const auto time = checkpoint["saved_at"].toInteger();
            age = Presentation::age(time);
            full = QDateTime::fromMSecsSinceEpoch(time).toLocalTime().toString(
                "yyyy-MM-dd HH:mm:ss t");
        } else {
            age = "Save time unknown";
            full = "Existing backup — save time unknown";
        }
    }
    load->setAge(age, full);
    progress->setVisible(busy);
    progress->setAccessibleName(op["action"].toObject()["type"].toString() + " in progress");
    progress->setAccessibleDescription(
        QString("%1 bytes copied; %2").arg(op["bytes_copied"].toInteger()).arg(str(op, "phase")));
    recovery->setVisible(op["status"] == "recovery_needed");
    recovery->setEnabled(connected && !busy);
    info->setText(Presentation::instructions(str(game, "info")));
    if (selected_ != selected) {
        selected_ = selected;
        setProperty("selected", selected);
        style()->unpolish(this);
        style()->polish(this);
        update();
    }
    details_->setVisible(selected);
    arrangeControls();
}

void MainWindow::applyTheme(bool dark) {
    QPalette palette;
    palette.setColor(QPalette::Window, QColor(dark ? "#191919" : "#f2f2f2"));
    palette.setColor(QPalette::WindowText, QColor(dark ? "#f0f0f0" : "#202020"));
    palette.setColor(QPalette::Base, QColor(dark ? "#101010" : "#ffffff"));
    palette.setColor(QPalette::Text, palette.color(QPalette::WindowText));
    palette.setColor(QPalette::Button, QColor(dark ? "#242424" : "#e8e8e8"));
    palette.setColor(QPalette::ButtonText, palette.color(QPalette::WindowText));
    palette.setColor(QPalette::PlaceholderText, QColor(dark ? "#b2b2b2" : "#626262"));
    palette.setColor(QPalette::Highlight, QColor("#9747ff"));
    palette.setColor(QPalette::HighlightedText, Qt::white);
    palette.setColor(QPalette::Disabled, QPalette::ButtonText,
                     QColor(dark ? "#727272" : "#989898"));
    palette.setColor(QPalette::Disabled, QPalette::PlaceholderText,
                     QColor(dark ? "#626262" : "#989898"));
    qApp->setPalette(palette);
    qApp->setStyleSheet(QString(R"(
        QWidget#gameRow { background:palette(window); }
        QWidget#gameRow:hover { background:%2; }
        QWidget#gameRow[selected="true"] { background:%3; }
        QWidget#footer { background:%2; }
        QWidget#footer { border-top:1px solid %1; }
        QPushButton { background:palette(button); border:1px solid %1; border-radius:4px; padding:4px 12px; }
        QPushButton:hover { background:%4; }
        QPushButton:focus { border-color:#9747ff; }
        QPushButton:disabled { color:palette(disabled,button-text); }
        QPushButton#gameTitle { background:transparent; border:0; padding:0; font-size:14px; font-weight:600; text-align:left; }
        QPushButton#gameTitle:focus { border-bottom:1px solid #9747ff; }
        QLabel#gameIcon { background:%4; border:1px solid %1; border-radius:5px; color:palette(placeholder-text); font-size:10px; }
        QLabel#gameStatus { color:palette(placeholder-text); }
        QLabel#gameStatus[running="true"] { color:%5; }
        QPushButton#more { background:transparent; border:0; padding:0; color:palette(placeholder-text); }
        QPushButton#more:hover { background:%4; }
        QPushButton#otherGames { text-align:left; border:0; border-top:1px dotted %1; border-radius:0; padding:10px 20px; background:transparent; color:palette(placeholder-text); }
        QPushButton#otherGames:hover { background:%2; }
        QPushButton#load { border-top-right-radius:0; border-bottom-right-radius:0; }
        QPushButton#historyArrow { border-top-left-radius:0; border-bottom-left-radius:0; padding:0; }
        QProgressBar { border:0; background:%1; }
        QProgressBar::chunk { background:#9747ff; }
        QScrollArea { border:0; background:transparent; }
        QLabel#keycap { background:%4; border-radius:3px; padding:2px 4px; color:palette(placeholder-text); }
        QLabel#error, QLabel#connection { color:%6; }
        QLabel#connection { padding:8px 18px; }
        QMenu, QWidget#historyPopup { background:palette(window); border:1px solid %1; }
        QMenu::item { padding:7px 16px; }
        QMenu::item:selected { background:%4; }
        QPushButton#historyAction { color:#9747ff; padding:3px 8px; }
        QPushButton#historyAction:disabled { color:palette(disabled,button-text); }
        QLabel#day { color:palette(placeholder-text); font-weight:600; padding-top:6px; }
        QLineEdit { padding:5px; border:1px solid %1; background:palette(base); }
        QCheckBox { spacing:6px; }
        QCheckBox::indicator { width:12px; height:12px; border:1px solid %1; border-radius:2px; }
        QCheckBox::indicator:checked { image:url(:/check.svg); background:#9747ff; border:1px solid #9747ff; }
    )")
                            .arg(dark ? "#3b3b3b" : "#cecece", dark ? "#1f1f1f" : "#eaeaea",
                                 dark ? "#101010" : "#dedede", dark ? "#303030" : "#d8d8d8",
                                 dark ? "#75e8b0" : "#167747", dark ? "#ffb2a9" : "#9d2525"));
}
MainWindow::MainWindow(Service *service, bool demo, QWidget *parent)
    : QMainWindow(parent), service_(service) {
    setWindowTitle(demo ? "Save Scummer — Demo" : "Save Scummer");
    setWindowIcon(QIcon(":/icon.svg"));
    setMinimumWidth(340);
    resize(620, 420);
    auto *central = new QWidget;
    auto *layout = new QVBoxLayout(central);
    layout->setContentsMargins(0, 0, 0, 0);
    layout->setSpacing(0);
    connection_ = label("Connecting to the background host…");
    connection_->setObjectName("connection");
    connection_->setWordWrap(true);
    layout->addWidget(connection_);
    auto *scroll = new QScrollArea;
    scroll_ = scroll;
    scroll->setWidgetResizable(true);
    scroll->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
    games_ = new QWidget;
    gamesLayout_ = new QVBoxLayout(games_);
    gamesLayout_->setContentsMargins(0, 0, 0, 0);
    gamesLayout_->setSpacing(0);
    gamesLayout_->setAlignment(Qt::AlignTop);
    empty_ = label("No known games installed on this computer.", games_);
    empty_->setContentsMargins(18, 20, 18, 20);
    empty_->setWordWrap(true);
    otherToggle_ = new QPushButton(games_);
    otherToggle_->setObjectName("otherGames");
    otherToggle_->setCheckable(true);
    connect(otherToggle_, &QPushButton::clicked, this, [this] { setOtherGamesOpen(!othersOpen_); });
    scroll->setWidget(games_);
    layout->addWidget(scroll, 1);
    auto *footer = new Footer(demo, this);
    footer_ = footer;
    sounds_ = footer->soundControl();
    startup_ = footer->startupControl();
    connect(startup_, &QCheckBox::clicked, this, [this](bool enabled) {
        startupSettingPending_ = true;
        startup_->setEnabled(false);
        service_->request({{"type", "set_launch_on_startup"}, {"enabled", enabled}}, [this](const auto &reply) {
            startupSettingPending_ = false;
            startup_->setChecked(state_["settings"].toObject()["launch_on_startup"].toBool());
            startup_->setEnabled(connected_);
            if (reply["type"] == "error") QMessageBox::warning(this, "Launch on startup", errorText(reply));
            else service_->request({{"type", "state"}}, [this](const auto &answer) {
                if (answer["type"] == "state") applyState(answer["state"].toObject());
            });
        });
    });
    layout->addWidget(footer);
    connect(sounds_, &QCheckBox::clicked, this, [this](bool enabled) {
        soundSettingPending_ = true;
        sounds_->setEnabled(false);
        service_->request(
            {{"type", "set_play_sounds"}, {"enabled", enabled}}, [this](const auto &reply) {
                soundSettingPending_ = false;
                sounds_->setChecked(state_["settings"].toObject()["play_sounds"].toBool(true));
                sounds_->setEnabled(connected_);
                if (reply["type"] == "error") {
                    QMessageBox::warning(this, "Play sounds", errorText(reply));
                } else {
                    service_->request({{"type", "state"}}, [this](const auto &answer) {
                        if (answer["type"] == "state")
                            applyState(answer["state"].toObject());
                    });
                }
            });
    });
    setCentralWidget(central);
    connect(service_, &Service::stateChanged, this, &MainWindow::applyState);
    connect(service_, &Service::connectionChanged, this, &MainWindow::setConnected);
    auto *clock = new QTimer(this);
    clock->setInterval(10000);
    connect(clock, &QTimer::timeout, this, &MainWindow::refresh);
    clock->start();
}
void MainWindow::resizeEvent(QResizeEvent *event) {
    QMainWindow::resizeEvent(event);
    if (event->size().width() != event->oldSize().width())
        scheduleFitHeight();
}
void MainWindow::scheduleFitHeight() {
    if (fitQueued_ || !state_.contains("games"))
        return;
    fitQueued_ = true;
    // Let visibility, word wrapping, and the wrapping footer settle first.
    QTimer::singleShot(0, this, [this] {
        fitQueued_ = false;
        if (isMaximized() || isFullScreen())
            return;
        centralWidget()->layout()->activate();
        const int contentWidth = scroll_->viewport()->width();
        const int rowsHeight = gamesLayout_->hasHeightForWidth()
                                   ? gamesLayout_->totalHeightForWidth(contentWidth)
                                   : gamesLayout_->sizeHint().height();
        const int noticeHeight = connection_->isHidden() ? 0 : connection_->heightForWidth(width());
        const int desired = rowsHeight + footer_->sizeHint().height() + qMax(0, noticeHeight);
        // Ordinary state/progress refreshes must not undo a user's height resize.
        if (desired == lastContentHeight_ && width() == lastFitWidth_)
            return;
        lastContentHeight_ = desired;
        lastFitWidth_ = width();
        const auto available = screen()->availableGeometry();
        const int decoration = frameGeometry().height() - height();
        const int limit = qMax(minimumSizeHint().height(), available.height() - decoration - 24);
        resize(width(), qBound(minimumSizeHint().height(), desired, limit));
        if (frameGeometry().bottom() > available.bottom())
            move(pos().x(),
                 qMax(available.top(), available.bottom() - frameGeometry().height() + 1));
    });
}
void MainWindow::setConnected(bool connected, const QString &message) {
    if (connected && !connected_)
        resetRevision_ = true;
    connected_ = connected;
    sounds_->setEnabled(connected && !soundSettingPending_);
    startup_->setEnabled(connected && !startupSettingPending_);
    connection_->setText(message);
    connection_->setVisible(!connected);
    if (!connected)
        submitting_.clear();
    refresh();
}
void MainWindow::applyState(const QJsonObject &state) {
    if (!resetRevision_ && state["revision"].toInteger() < state_["revision"].toInteger())
        return;
    resetRevision_ = false;
    state_ = state;
    if (connected_) {
        QStringList diagnostics;
        for (const auto &value : state_["discovery_errors"].toArray()) diagnostics.append(value.toString());
        connection_->setText(diagnostics.join("\n"));
        connection_->setVisible(!diagnostics.isEmpty());
    }
    if (!soundSettingPending_)
        sounds_->setChecked(state_["settings"].toObject()["play_sounds"].toBool(true));
    if (!startupSettingPending_)
        startup_->setChecked(state_["settings"].toObject()["launch_on_startup"].toBool());
    const auto operations = state_["operations"].toObject();
    for (const auto &value : operations) {
        const auto op = value.toObject();
        const auto game = str(op, "game_id");
        if (op["status"] == "pending")
            submitting_.remove(game);
        if (op["status"] == "failed" && op["error"].isObject()) {
            // Latest terminal result per game, not an old failure resurfacing forever.
            bool newer = false;
            for (const auto &candidate : operations)
                if (candidate.toObject()["game_id"] == game &&
                    candidate.toObject()["started_at"].toInteger() > op["started_at"].toInteger())
                    newer = true;
            if (!newer)
                errors_[game] = op["error"].toObject()["message"].toString();
        }
    }
    refresh();
}
void MainWindow::refresh() {
    scheduleFitHeight();
    const auto order = Presentation::orderedGames(state_);
    QStringList running;
    for (const auto &id : state_["active_stack"].toArray())
        if (order.contains(id.toString()))
            running.append(id.toString());
    if (!order.contains(selected_))
        selected_ = order.value(0);
    if (!running.isEmpty() && !hadRunning_)
        othersOpen_ = !running.contains(selected_);
    if (!running.isEmpty() && !selected_.isEmpty() && !running.contains(selected_))
        othersOpen_ = true;
    hadRunning_ = !running.isEmpty();
    for (auto it = rows_.begin(); it != rows_.end();) {
        if (!order.contains(it.key())) {
            if (historyGame_ == it.key())
                closePopup();
            delete it.value();
            it = rows_.erase(it);
        } else
            ++it;
    }
    while (auto *item = gamesLayout_->takeAt(0))
        delete item;
    empty_->setVisible(order.isEmpty());
    gamesLayout_->addWidget(empty_);
    const bool showOther = !running.isEmpty() && order.size() > running.size();
    otherToggle_->setVisible(showOther);
    otherToggle_->setText(QString("  Other games   %1").arg(order.size() - running.size()));
    otherToggle_->setIcon(QIcon(othersOpen_ ? ":/chevron.svg" : ":/chevron-right.svg"));
    otherToggle_->setChecked(othersOpen_);
    otherToggle_->setAccessibleName("Other games");
    bool inserted = false;
    for (const auto &id : order) {
        if (!running.contains(id) && showOther && !inserted) {
            gamesLayout_->addWidget(otherToggle_);
            inserted = true;
        }
        if (!rows_.contains(id)) {
            auto *row = new GameRow(id, games_);
            rows_[id] = row;
            connect(row, &GameRow::selected, this, [this, id] { selectGame(id); });
            connect(row, &GameRow::action, this,
                    [this, id](const auto &action) { execute(id, action); });
            connect(row, &GameRow::showHistory, this, [this, id] { history(id); });
            connect(row, &GameRow::showOptions, this, [this, id] { options(id); });
            connect(row, &GameRow::showRecovery, this, [this, id] { recover(id); });
        }
        auto *row = rows_[id];
        gamesLayout_->addWidget(row);
        row->updateState(state_, id == selected_, connected_, submitting_.contains(id));
        const auto configurationError = gameOf(state_, id)["configuration_error"].toString();
        const auto error = configurationError.isEmpty() ? errors_.value(id) : configurationError;
        row->error->setText(error);
        row->error->setVisible(!error.isEmpty());
        row->setVisible(!showOther || running.contains(id) || othersOpen_);
    }
    if (historyBody_)
        populateHistory();
    if (auto *menu = qobject_cast<QMenu *>(popup_.data())) {
        const auto game = menu->property("game").toString();
        const bool idle = connected_ && !submitting_.contains(game) &&
                          Presentation::blockingOperation(state_, game).isEmpty();
        for (auto *action : menu->actions())
            if (action->property("requiresIdle").toBool())
                action->setEnabled(idle && action->property("available").toBool());
    }
    for (auto *dialog : findChildren<QDialog *>()) {
        if (!dialog->property("configuredGame").isValid())
            continue;
        const auto game = dialog->property("configuredGame").toString();
        if (auto *save = dialog->findChild<QPushButton *>("configureSave"))
            save->setEnabled(connected_ && !dialog->property("submitting").toBool() &&
                             !submitting_.contains(game) &&
                             Presentation::blockingOperation(state_, game).isEmpty());
    }
}
void MainWindow::selectGame(const QString &id) {
    if (selected_ == id || !rows_.contains(id))
        return;
    selected_ = id;
    closePopup();
    refresh();
}
void MainWindow::setOtherGamesOpen(bool open) {
    othersOpen_ = open;
    closePopup();
    if (!open && !state_["active_stack"].toArray().contains(selected_)) {
        const auto order = Presentation::orderedGames(state_);
        selected_ = order.value(0);
    }
    refresh();
}
void MainWindow::closePopup() {
    historyBody_.clear();
    historyGame_.clear();
    if (popup_) {
        popup_->close();
        popup_->deleteLater();
        popup_.clear();
    }
}
void MainWindow::showError(const QString &game, const QString &message) {
    errors_[game] = message;
    refresh();
}
void MainWindow::execute(const QString &game, const QJsonObject &action) {
    if (!connected_ || submitting_.contains(game))
        return;
    selectGame(game);
    submitting_.insert(game);
    errors_.remove(game);
    refresh();
    service_->request({{"type", "execute"}, {"game_id", game}, {"action", action}},
                      [this, game](const auto &reply) {
                          submitting_.remove(game);
                          if (reply["type"] == "error")
                              showError(game, errorText(reply));
                          else if (reply["type"] == "accepted") {
                              // An accepted response can precede its first watch update. Keep
                              // controls locked until a fresh state query has observed the durable
                              // operation.
                              submitting_.insert(game);
                              service_->request({{"type", "state"}},
                                                [this, game](const auto &answer) {
                                                    submitting_.remove(game);
                                                    if (answer["type"] == "state")
                                                        applyState(answer["state"].toObject());
                                                    else
                                                        showError(game, errorText(answer));
                                                });
                          }
                          refresh();
                      });
}
void MainWindow::placePopup(QWidget *popup, QWidget *anchor) {
    const auto point = anchor->mapToGlobal(QPoint(0, anchor->height() + 3));
    const auto screen = anchor->screen()->availableGeometry();
    popup->adjustSize();
    popup->resize(qMin(popup->width(), screen.width() - 12),
                  qMin(popup->height(), qMax(80, screen.bottom() - point.y() - 6)));
    popup->move(qBound(screen.left() + 6, point.x(), screen.right() - popup->width() - 6),
                qMin(point.y(), screen.bottom() - popup->height() - 6));
    popup->show();
    popup->setFocus();
}
void MainWindow::history(const QString &game) {
    if (popup_ && historyGame_ == game && popup_->isVisible()) {
        closePopup();
        return;
    }
    closePopup();
    historyGame_ = game;
    auto *popup = new QWidget(this, Qt::Popup);
    popup_ = popup;
    popup->setObjectName("historyPopup");
    auto *layout = new QVBoxLayout(popup);
    layout->setContentsMargins(4, 4, 4, 4);
    auto *scroll = new QScrollArea;
    scroll->setWidgetResizable(true);
    scroll->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
    historyBody_ = new QWidget;
    new QVBoxLayout(historyBody_);
    scroll->setWidget(historyBody_);
    layout->addWidget(scroll);
    const auto below = rows_[game]->arrow->mapToGlobal(QPoint(0, rows_[game]->arrow->height() + 3));
    const int availableHeight =
        rows_[game]->screen()->availableGeometry().bottom() - below.y() - 16;
    scroll->setFixedSize(qMin(390, screen()->availableGeometry().width() - 24),
                         qMin(350, qMax(60, availableHeight)));
    populateHistory();
    placePopup(popup, rows_[game]->load);
    service_->request({{"type", "history"}, {"game_id", game}}, [this, game](const auto &reply) {
        if (reply["type"] == "state")
            applyState(reply["state"].toObject());
        else if (reply["type"] == "error")
            showError(game, errorText(reply));
    });
}
void MainWindow::populateHistory() {
    if (!historyBody_)
        return;
    auto *layout = qobject_cast<QVBoxLayout *>(historyBody_->layout());
    while (auto *item = layout->takeAt(0)) {
        if (auto *widget = item->widget()) {
            widget->hide();
            widget->setParent(nullptr);
            widget->deleteLater();
        }
        delete item;
    }
    layout->setContentsMargins(8, 4, 8, 4);
    layout->setSpacing(4);
    const auto rows = Presentation::history(state_, historyGame_);
    const auto snapshots = state_["snapshots"].toObject();
    const bool enabled = connected_ && !submitting_.contains(historyGame_) &&
                         Presentation::blockingOperation(state_, historyGame_).isEmpty();
    QString previousDay;
    const QMap<QString, QString> names{{"saved", "Saved"},
                                       {"existing_backup", "Existing backup"},
                                       {"loaded", "Loaded"},
                                       {"reverted", "Reverted"},
                                       {"game_started", "Game started"},
                                       {"game_closed", "Game closed"}};
    for (const auto &row : rows) {
        const bool existing = row["kind"] == "existing_backup";
        const auto group =
            existing ? QString("Other backups") : Presentation::day(row["recorded_at"].toInteger());
        if (group != previousDay) {
            auto *day = label(group);
            day->setObjectName("day");
            layout->addWidget(day);
            previousDay = group;
        }
        auto *widget = new QWidget;
        auto *line = new QHBoxLayout(widget);
        line->setContentsMargins(0, 3, 0, 3);
        line->setSpacing(8);
        auto *time = label(existing ? "—"
                                    : QDateTime::fromMSecsSinceEpoch(row["recorded_at"].toInteger())
                                          .toLocalTime()
                                          .toString("HH:mm:ss"));
        line->addWidget(time);
        QString caption = names.value(str(row, "kind"), str(row, "kind"));
        if (row["target_id"].isString())
            for (const auto &target : state_["history"].toArray()) {
                if (target.toObject()["id"] == row["target_id"])
                    caption +=
                        " [" +
                        QDateTime::fromMSecsSinceEpoch(target.toObject()["recorded_at"].toInteger())
                            .toLocalTime()
                            .toString("dd MMM, HH:mm:ss") +
                        "]";
            }
        const auto action = Presentation::historyAction(row);
        const bool available =
            snapshots[action["target"].toString()].toObject()["available"].toBool();
        if (existing)
            caption += "\nSave time unknown";
        if (!action.isEmpty() && !available)
            caption += "\nBackup unavailable";
        auto *text = label(caption);
        text->setWordWrap(true);
        line->addWidget(text, 1);
        if (!action.isEmpty()) {
            auto *button = new QPushButton(action["type"] == "load" ? "Restore" : "Revert");
            button->setObjectName("historyAction");
            button->setProperty("checkpoint", action["target"]);
            button->setEnabled(enabled && available);
            button->setAccessibleName(button->text() + " " + caption + " " + time->text());
            const auto game = historyGame_;
            connect(button, &QPushButton::clicked, this,
                    [this, game, action] { execute(game, action); });
            line->addWidget(button);
        }
        layout->addWidget(widget);
    }
    layout->addStretch();
}
void MainWindow::options(const QString &game) {
    closePopup();
    auto *menu = new QMenu(this);
    popup_ = menu;
    menu->setProperty("game", game);
    const bool idle = connected_ && !submitting_.contains(game) &&
                      Presentation::blockingOperation(state_, game).isEmpty();
    menu->addAction("Explore", this, [this, game] {
        service_->request({{"type", "explore"}, {"game_id", game}}, [this, game](const auto &reply) {
            if (reply["type"] == "error") showError(game, errorText(reply));
        });
    });
    auto *configureAction = menu->addAction("Configure…", this, [this, game] { configure(game); });
    configureAction->setEnabled(idle);
    configureAction->setProperty("requiresIdle", true);
    configureAction->setProperty("available", true);
    bool any = !Presentation::history(state_, game).isEmpty();
    for (const auto &value : state_["snapshots"].toObject())
        if (value.toObject()["game_id"] == game && value.toObject()["removed_at"].isNull())
            any = true;
    auto *flushAction = menu->addAction("Flush history…", this, [this, game] { flush(game); });
    flushAction->setEnabled(idle && any);
    flushAction->setProperty("requiresIdle", true);
    flushAction->setProperty("available", any);
    placePopup(menu, rows_[game]->more);
}
void MainWindow::configure(const QString &id) {
    closePopup();
    const auto game = gameOf(state_, id);
    auto *dialog = new QDialog(this);
    dialog->setAttribute(Qt::WA_DeleteOnClose);
    dialog->setWindowTitle("Configure " + str(game, "name"));
    dialog->setProperty("configuredGame", id);
    auto *form = new QFormLayout(dialog);
    const auto paths = game["executables"].toArray();
    auto *executable = new QLineEdit(paths.isEmpty() ? QString() : paths.first().toString());
    auto *directory = new QLineEdit(str(game, "data_dir"));
    const auto locations = game["detected_locations"].toArray();
    auto *choices = new QComboBox;
    choices->addItem("Current configuration", QJsonObject{});
    for (const auto &value : locations) {
        const auto location = value.toObject();
        choices->addItem(location["executables"].toArray().first().toString() + " — " + location["data_dir"].toString(), location);
    }
    if (!locations.isEmpty()) form->addRow("Detected locations:", choices);
    else choices->setParent(dialog);
    connect(choices, &QComboBox::currentIndexChanged, dialog, [choices, executable, directory](int index) {
        if (index <= 0) return;
        const auto location = choices->currentData().toJsonObject();
        executable->setText(location["executables"].toArray().first().toString());
        directory->setText(location["data_dir"].toString());
    });
    auto addPath = [this, dialog, form, choices, locations](const QString &caption, QLineEdit *edit, bool folder) {
        auto *row = new QWidget;
        auto *layout = new QHBoxLayout(row);
        layout->setContentsMargins(0, 0, 0, 0);
        layout->addWidget(edit);
        auto *browse = new QPushButton("Browse…");
        auto *reset = new QPushButton("Reset");
        layout->addWidget(browse);
        layout->addWidget(reset);
        reset->setEnabled(!locations.isEmpty());
        reset->setToolTip("Use the selected detected location, or the sole catalog default.");
        QObject::connect(reset, &QPushButton::clicked, dialog,
                         [edit, folder, choices, locations] {
                             auto location = choices->currentData().toJsonObject();
                             if (location.isEmpty() && locations.size() == 1) location = locations.first().toObject();
                             if (location.isEmpty()) { choices->showPopup(); return; }
                             edit->setText(folder ? location["data_dir"].toString() : location["executables"].toArray().first().toString());
                         });
        QObject::connect(browse, &QPushButton::clicked, dialog, [dialog, edit, folder] {
            const auto path =
                folder
                    ? QFileDialog::getExistingDirectory(dialog, "Game data directory", edit->text())
                    : QFileDialog::getOpenFileName(dialog, "Game executable", edit->text());
            if (!path.isEmpty())
                edit->setText(QDir::toNativeSeparators(path));
        });
        form->addRow(caption, row);
    };
    addPath("Game executable:", executable, false);
    addPath("Game data dir (DIR):", directory, true);
    auto *error = label("");
    error->setObjectName("error");
    error->setWordWrap(true);
    form->addRow(error);
    auto *buttons = new QDialogButtonBox(QDialogButtonBox::Save | QDialogButtonBox::Cancel);
    form->addRow(buttons);
    buttons->button(QDialogButtonBox::Save)->setObjectName("configureSave");
    connect(buttons, &QDialogButtonBox::rejected, dialog, &QDialog::reject);
    const QPointer<QDialog> guard(dialog);
    connect(buttons, &QDialogButtonBox::accepted, dialog,
            [this, guard, game, id, directory, executable, error, buttons, choices] {
                if (!guard)
                    return;
                guard->setProperty("submitting", true);
                buttons->button(QDialogButtonBox::Save)->setEnabled(false);
                auto executables = game["executables"].toArray();
                if (choices->currentIndex() > 0) executables = choices->currentData().toJsonObject()["executables"].toArray();
                if (executables.isEmpty()) {
                    if (!executable->text().isEmpty())
                        executables.append(executable->text());
                } else
                    executables.replace(0, executable->text());
                service_->request({{"type", "configure"},
                                   {"id", id},
                                   {"name", game["name"]},
                                   {"data_dir", directory->text()},
                                   {"executables", executables}},
                                  [guard, error, buttons](const auto &reply) {
                                      if (!guard)
                                          return;
                                      guard->setProperty("submitting", false);
                                      if (reply["type"] == "configured")
                                          guard->accept();
                                      else {
                                          error->setText(errorText(reply));
                                          buttons->button(QDialogButtonBox::Save)->setEnabled(true);
                                      }
                                  });
            });
    dialog->resize(640, 180);
    dialog->open();
}
void MainWindow::flush(const QString &game) {
    closePopup();
    service_->request({{"type", "flush_preview"}, {"game_id", game}}, [this,
                                                                       game](const auto &reply) {
        if (reply["type"] != "flush_preview") {
            showError(game, errorText(reply));
            return;
        }
        const auto preview = reply["preview"].toObject();
        auto *box =
            new QMessageBox(QMessageBox::Warning, "Flush history",
                            QString("This will permanently delete %1 saved backups and %2 recovery "
                                    "points, and clear this game's history. Current game data will "
                                    "be kept.\n\nRetained incomplete copies: %3.")
                                .arg(preview["saved"].toInteger())
                                .arg(preview["recovery"].toInteger())
                                .arg(preview["retained"].toInteger()),
                            QMessageBox::Yes | QMessageBox::Cancel, this);
        QStringList paths;
        for (const auto &path : preview["paths"].toArray())
            paths.append(path.toString());
        box->setDetailedText(paths.join('\n'));
        box->setDefaultButton(QMessageBox::Cancel);
        box->setAttribute(Qt::WA_DeleteOnClose);
        box->button(QMessageBox::Yes)->setText("Delete backups");
        connect(box, &QMessageBox::finished, this, [this, game, preview](int result) {
            if (result == QMessageBox::Yes)
                execute(game, {{"type", "flush"}, {"confirmed_revision", preview["revision"]}});
        });
        box->open();
    });
}
void MainWindow::recover(const QString &game) {
    const auto op = Presentation::blockingOperation(state_, game);
    if (op["status"] != "recovery_needed")
        return;
    auto *box =
        new QMessageBox(QMessageBox::Warning, "Recovery needed",
                        "An interrupted operation needs a decision. Keep the current game data, "
                        "restore the pre-operation data, or retry recovery. Retained files remain "
                        "available until recovery is resolved and history is flushed.",
                        QMessageBox::Cancel, this);
    box->setInformativeText(op["error"].toObject()["message"].toString());
    box->setDetailedText("Current data: " + str(op, "live") + "\nRecovery: " + str(op, "recovery") +
                         "\nOriginal: " + str(op, "original") + "\nStaging: " + str(op, "staging"));
    const QMap<QString, QString> choices{{"Keep current data", "keep_current"},
                                         {"Restore before operation", "restore_before"},
                                         {"Retry recovery", "retry"}};
    for (auto it = choices.begin(); it != choices.end(); ++it) {
        auto *button = box->addButton(it.key(), QMessageBox::ActionRole);
        const auto choice = it.value();
        connect(button, &QPushButton::clicked, this, [this, game, op, choice] {
            execute(game, {{"type", "recover"}, {"operation", op["id"]}, {"choice", choice}});
        });
    }
    box->setAttribute(Qt::WA_DeleteOnClose);
    box->open();
}
