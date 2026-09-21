#include "mainwindow.h"
#include <QScrollBar>
#include "presentation.h"
#include <QApplication>
#include <QCheckBox>
#include <QDesktopServices>
#include <QComboBox>
#include <QDialogButtonBox>
#include <QFileDialog>
#include <QFileInfo>
#include <QFormLayout>
#include <QFrame>
#include <QGridLayout>
#include <QIconEngine>
#include <QImage>
#include <QKeyEvent>
#include <QLineEdit>
#include <QLocale>
#include <QMenu>
#include <QMessageBox>
#include <QPainter>
#include <QPlainTextEdit>
#include <QScreen>
#include <QShortcut>
#include <QResizeEvent>
#include <QStyleOptionButton>
#include <QSvgRenderer>
#include <QToolButton>
#include <QTextLayout>
#include <QUrl>
#include <QWidgetAction>
#include <QtMath>
#include <functional>
#ifdef Q_OS_WIN
#define NOMINMAX
#include <windows.h>
#endif

namespace {
constexpr int CompactContentWidth = 500;
QMargins contentInsets(int width) {
    return width < CompactContentWidth ? QMargins(14, 10, 14, 10)
                                       : QMargins(18, 12, 18, 12);
}
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
void setDialogDefault(QDialogButtonBox *buttons, QPushButton *defaultButton) {
    for (auto *abstractButton : buttons->buttons()) {
        auto *button = qobject_cast<QPushButton *>(abstractButton);
        if (!button)
            continue;
        button->setIcon({});
        button->setAutoDefault(false);
        button->setDefault(false);
    }
    defaultButton->setDefault(true);
}
int historyTimeWidth(const QFont &font) {
    const QFontMetrics metrics(font);
    QStringList labels{"59 seconds ago", "59 minutes ago", "23 hours and 59 minutes ago",
                       "Yesterday", "0000-00-00"};
    for (int day = 1; day <= 7; ++day)
        labels.append(QLocale().dayName(day, QLocale::LongFormat));
    int width = 0;
    for (const auto &text : labels)
        width = qMax(width, metrics.horizontalAdvance(text));
    return width + 8;
}
QJsonObject gameOf(const QJsonObject &state, const QString &id) {
    return state["games"].toObject()[id].toObject();
}
int layoutCaption(QTextLayout &layout, int width) {
    QTextOption option;
    option.setAlignment(Qt::AlignHCenter);
    option.setWrapMode(QTextOption::WrapAtWordBoundaryOrAnywhere);
    layout.setTextOption(option);
    qreal height = 0;
    layout.beginLayout();
    while (true) {
        auto line = layout.createLine();
        if (!line.isValid()) break;
        line.setLineWidth(qMax(1, width));
        line.setPosition(QPointF(0, height));
        height += line.height();
    }
    layout.endLayout();
    return qCeil(height);
}

class PaletteIconEngine final : public QIconEngine {
  public:
    explicit PaletteIconEngine(QString source) : source_(std::move(source)) {}
    QIconEngine *clone() const override { return new PaletteIconEngine(source_); }
    void paint(QPainter *painter, const QRect &rect, QIcon::Mode mode,
               QIcon::State state) override {
        const qreal scale = painter->device()->devicePixelRatioF();
        painter->drawPixmap(rect, render(rect.size(), scale, mode, state));
    }
    QPixmap pixmap(const QSize &size, QIcon::Mode mode, QIcon::State state) override {
        return render(size, 1.0, mode, state);
    }
    QPixmap scaledPixmap(const QSize &size, QIcon::Mode mode, QIcon::State state,
                         qreal scale) override {
        return render(size, scale, mode, state);
    }

  private:
    QPixmap render(const QSize &size, qreal scale, QIcon::Mode mode, QIcon::State) const {
        const QSize pixels(qMax(1, qRound(size.width() * scale)),
                           qMax(1, qRound(size.height() * scale)));
        QImage image(pixels, QImage::Format_ARGB32_Premultiplied);
        image.fill(Qt::transparent);
        QSvgRenderer renderer(source_);
        QPainter imagePainter(&image);
        renderer.render(&imagePainter, QRectF(QPointF(0, 0), QSizeF(pixels)));
        imagePainter.setCompositionMode(QPainter::CompositionMode_SourceIn);
        const auto group = mode == QIcon::Disabled ? QPalette::Disabled : QPalette::Active;
        imagePainter.fillRect(image.rect(), qApp->palette().color(group, QPalette::ButtonText));
        imagePainter.end();
        auto pixmap = QPixmap::fromImage(image);
        pixmap.setDevicePixelRatio(scale);
        return pixmap;
    }

    QString source_;
};

QIcon paletteIcon(const QString &source) {
    return QIcon(new PaletteIconEngine(source));
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
class SquareButton : public QPushButton {
  public:
    using QPushButton::QPushButton;
    QSize sizeHint() const override {
        const int side = QPushButton::sizeHint().height();
        return {side, side};
    }
};
class IconMenuRow : public QPushButton {
  public:
    static constexpr int ItemHeight = 36;
    IconMenuRow(QAction *action, QMenu *menu) : QPushButton(menu), action_(action) {
        setObjectName("iconMenuItem");
        setFixedHeight(ItemHeight);
        setCursor(action->isEnabled() ? Qt::PointingHandCursor : Qt::ForbiddenCursor);
        setAccessibleName(action->text());
        auto *line = new QHBoxLayout(this);
        line->setContentsMargins(0, 0, 16, 0);
        line->setSpacing(6);
        icon_ = label("", this);
        icon_->setObjectName("iconMenuIcon");
        icon_->setFixedSize(ItemHeight, ItemHeight);
        icon_->setAlignment(Qt::AlignCenter);
        icon_->setAttribute(Qt::WA_TransparentForMouseEvents);
        text_ = label(action->text(), this);
        text_->setAttribute(Qt::WA_TransparentForMouseEvents);
        line->addWidget(icon_);
        line->addWidget(text_, 1);
        setMinimumWidth(ItemHeight + fontMetrics().horizontalAdvance(action->text()) + 46);
        const auto sync = [this] {
            setEnabled(action_->isEnabled());
            setToolTip(action_->toolTip());
            setCursor(action_->isEnabled() ? Qt::PointingHandCursor : Qt::ForbiddenCursor);
            icon_->setPixmap(action_->icon().pixmap(
                QSize(18, 18), action_->isEnabled() ? QIcon::Normal : QIcon::Disabled));
        };
        sync();
        connect(action, &QAction::changed, this, sync);
        connect(this, &QPushButton::clicked, menu, [action, menu] {
            menu->close();
            action->trigger();
        });
    }

  private:
    QAction *action_;
    QLabel *icon_, *text_;
};
class IconMenu : public QMenu {
  public:
    explicit IconMenu(QWidget *parent = nullptr) : QMenu(parent) { setObjectName("iconMenu"); }
    QAction *addIconAction(const QIcon &icon, const QString &text,
                           std::function<void()> callback) {
        auto *action = new QWidgetAction(this);
        action->setIcon(icon);
        action->setText(text);
        action->setDefaultWidget(new IconMenuRow(action, this));
        connect(action, &QAction::triggered, this,
                [callback = std::move(callback)] { callback(); });
        addAction(action);
        return action;
    }
    void addIconSeparator() {
        auto *action = new QWidgetAction(this);
        auto *separator = new QFrame(this);
        separator->setObjectName("iconMenuSeparator");
        separator->setFrameShape(QFrame::HLine);
        separator->setFixedHeight(7);
        action->setDefaultWidget(separator);
        addAction(action);
    }
    int iconColumnCenter() const {
        return style()->pixelMetric(QStyle::PM_MenuPanelWidth) + IconMenuRow::ItemHeight / 2;
    }
};
class DisabledButtonCursorFilter : public QObject {
  public:
    using QObject::QObject;
    bool eventFilter(QObject *watched, QEvent *event) override {
        auto *button = qobject_cast<QPushButton *>(watched);
        if (button && (event->type() == QEvent::EnabledChange ||
                       event->type() == QEvent::Show || event->type() == QEvent::Enter)) {
            if (!button->isEnabled())
                button->setCursor(Qt::ForbiddenCursor);
            else if (button->cursor().shape() == Qt::ForbiddenCursor)
                button->unsetCursor();
        }
        return false;
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
        hints_->setToolTip("When this window is focused, shortcuts use the selected game's Save and Load buttons. "
                          "Otherwise, global shortcuts target the top running game.");
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
    setAccessibleName(age.isEmpty() ? "Load" : "Load, " + (full.isEmpty() ? age : full));
    updateGeometry();
    update();
}
QFont LoadButton::captionFont() const {
    auto smaller = font();
    if (smaller.pointSizeF() > 0)
        smaller.setPointSizeF(qMax(8.0, smaller.pointSizeF() - 1));
    else
        smaller.setPixelSize(qMax(1, smaller.pixelSize() - 1));
    return smaller;
}
QSize LoadButton::sizeHint() const {
    // A font-scaled preferred width, independent of the current age caption.
    const int width = qMax(QPushButton::sizeHint().width(),
                          QFontMetrics(captionFont()).horizontalAdvance("yyyy-MM-dd, HH:mm:ss") + 24);
    return {width, heightForWidth(width)};
}
int LoadButton::heightForWidth(int width) const {
    QTextLayout caption(age_, captionFont());
    const int captionHeight = qMax(2 * QFontMetrics(captionFont()).height(),
                                  layoutCaption(caption, width - 12));
    const int titleHeight = qMax(fontMetrics().height(), iconSize().height());
    return qMax(QPushButton::sizeHint().height(), titleHeight + captionHeight + 12);
}
void LoadButton::paintEvent(QPaintEvent *) {
    QStyleOptionButton option;
    initStyleOption(&option);
    option.text.clear();
    option.icon = {};
    QPainter painter(this);
    style()->drawControl(QStyle::CE_PushButton, &option, &painter, this);
    const auto drawTitle = [this, &painter](const QRect &area) {
        QStyleOptionButton title;
        initStyleOption(&title);
        title.rect = area;
        title.text = "Load";
        style()->drawControl(QStyle::CE_PushButtonLabel, &title, &painter, this);
    };
    if (age_.isEmpty()) {
        drawTitle(rect());
        return;
    }
    QTextLayout caption(age_, captionFont());
    const int captionHeight = layoutCaption(caption, width() - 12);
    const int titleHeight = qMax(fontMetrics().height(), iconSize().height());
    const int top = (height() - titleHeight - 2 - captionHeight) / 2;
    drawTitle(QRect(6, top, width() - 12, titleHeight));
    painter.setPen(palette().color(isEnabled() ? QPalette::Active : QPalette::Disabled,
                                   QPalette::PlaceholderText));
    caption.draw(&painter, QPointF(6, top + titleHeight + 2));
}
GameRow::GameRow(const QString &gameId, QWidget *parent) : QWidget(parent), id(gameId) {
    setObjectName("gameRow");
    setAttribute(Qt::WA_StyledBackground);
    setSizePolicy(QSizePolicy::Expanding, QSizePolicy::Maximum);
    auto *layout = new QVBoxLayout(this);
    layout->setContentsMargins(contentInsets(width()));
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
    save->setIcon(paletteIcon(":/icons/save.svg"));
    save->setIconSize(QSize(24, 24));
    split_ = new QWidget(pair_);
    load = new LoadButton("Load", split_);
    load->setObjectName("load");
    load->setIcon(paletteIcon(":/icons/load.svg"));
    load->setIconSize(QSize(24, 24));
    arrow = new QPushButton(split_);
    arrow->setIcon(paletteIcon(":/icons/chevron-down.svg"));
    arrow->setObjectName("historyArrow");
    arrow->setAccessibleName("Load history");
    arrow->setToolTip("Load history");
    more = new QPushButton(controls_);
    more->setIcon(paletteIcon(":/icons/ellipsis.svg"));
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
    info->setTextInteractionFlags(Qt::NoTextInteraction);
    info->setCursor(Qt::ArrowCursor);
    info->setSizePolicy(QSizePolicy::Ignored, QSizePolicy::Preferred);
    detailsLayout->addWidget(info);
    error = label("");
    error->setObjectName("error");
    error->setWordWrap(true);
    error->hide();
    detailsLayout->addWidget(error);
    recovery = new QPushButton("Recovery needed…");
    recovery->setObjectName("recovery");
    recovery->setIcon(paletteIcon(":/icons/warning.svg"));
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
    layout()->setContentsMargins(contentInsets(width()));
    arrangeControls();
}
void GameRow::arrangeControls() {
    const int arrowWidth = qMax(28, arrow->fontMetrics().height() + 12);
    const int preferred = qMax(save->sizeHint().width(), load->sizeHint().width() + arrowWidth);
    const int inset = width() < 500 ? 28 : 36;
    const int available = qMax(40, width() - inset - arrowWidth - 6);
    const int equalWidth = qMax(20, qMin(preferred, (available - 6) / 2));
    const int height = qMax(save->sizeHint().height(), load->heightForWidth(qMax(1, equalWidth - arrowWidth)));
    controls_->setFixedHeight(height);
    pair_->setGeometry(0, 0, equalWidth * 2 + 6, height);
    save->setGeometry(0, 0, equalWidth, height);
    split_->setGeometry(equalWidth + 6, 0, equalWidth, height);
    load->setGeometry(0, 0, qMax(1, equalWidth - arrowWidth), height);
    arrow->setGeometry(qMax(1, equalWidth - arrowWidth), 0, arrowWidth, height);
    more->setGeometry(pair_->width() + 6, 0, arrowWidth, height);
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
    const bool hasHistory = state["history_status"].toObject()[id].toObject()["has_visible_history"].toBool();
    const auto op = Presentation::blockingOperation(state, id);
    const bool busy = submitting || op["status"] == "pending";
    const bool blocked =
        !connected || busy || !op.isEmpty() || game["configuration_error"].isString();
    const bool uninstalled = game["origin"] == "custom" && !game["installed"].toBool();
    status_->setText(op["status"] == "recovery_needed"                          ? "Recovery needed"
                     : running                                                  ? "Running"
                     : uninstalled                                              ? "Uninstalled"
                     : !dataAvailable && !hasHistory && checkpoint.isEmpty() ? "Not run yet"
                                                                                : "");
    status_->setProperty("running", running);
    status_->style()->unpolish(status_);
    status_->style()->polish(status_);
    save->setEnabled(!blocked && dataAvailable);
    load->setEnabled(!blocked && dataAvailable && !checkpoint.isEmpty());
    arrow->setEnabled(connected && !busy && hasHistory);
    more->setEnabled(true);
    QString age, full;
    if (!checkpoint.isEmpty()) {
        const bool saved = checkpoint["saved_at"].isDouble();
        const auto timestamp = saved ? checkpoint["saved_at"] : checkpoint["selection_time"];
        if (timestamp.isDouble()) {
            const auto time = timestamp.toInteger();
            age = Presentation::age(time);
            full = QDateTime::fromMSecsSinceEpoch(time).toLocalTime().toString(
                "yyyy-MM-dd HH:mm:ss t");
            if (!saved) {
                age = "Modified " + age;
                full = "Folder modified: " + full;
            }
        } else {
            age = "Save time unknown";
            full = "Existing backup — save time unknown";
        }
    }
    if (checkpoint.isEmpty()) {
        age = "No checkpoints saved";
        full = age;
    }
    load->setAge(age, full);
    progress->setVisible(busy);
    progress->setAccessibleName(op["action"].toObject()["type"].toString() + " in progress");
    progress->setAccessibleDescription(
        QString("%1 bytes copied; %2").arg(op["bytes_copied"].toInteger()).arg(str(op, "phase")));
    recovery->setVisible(op["status"] == "recovery_needed");
    recovery->setEnabled(connected && !busy);
    for (auto *button : {save, static_cast<QPushButton *>(load), arrow, recovery})
        button->setCursor(button->isEnabled() ? Qt::ArrowCursor : Qt::ForbiddenCursor);
    // Disabled child widgets can be skipped during cursor lookup on some Qt
    // platforms, so mirror Load's cursor on the container directly behind it.
    // The enabled history arrow still supplies its own cursor over its segment.
    split_->setCursor(load->isEnabled() ? Qt::ArrowCursor : Qt::ForbiddenCursor);
    info->setText(Presentation::instructions(str(game, "info")));
    if (selected_ != selected) {
        selected_ = selected;
        setProperty("selected", selected);
        style()->unpolish(this);
        style()->polish(this);
        update();
    }
    setCursor(selected ? Qt::ArrowCursor : Qt::PointingHandCursor);
    header->setCursor(selected ? Qt::ArrowCursor : Qt::PointingHandCursor);
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
        QPushButton { background:palette(button); border:2px solid %1; border-radius:5px; padding:3px 11px; }
        QPushButton:hover { background:%4; }
        QPushButton:default { background:#9747ff; border-color:%1; color:white; }
        QPushButton:default:hover { background:#8435e8; }
        QPushButton:focus { border-color:%7; }
        QPushButton:disabled { color:palette(disabled,button-text); }
        QPushButton:default:disabled { background:palette(button); border-color:%1; color:palette(disabled,button-text); }
        QPushButton#gameTitle { background:transparent; border:0; padding:0; font-size:14px; font-weight:600; text-align:left; }
        QPushButton#gameTitle:focus { border-bottom:1px solid #9747ff; }
        QLabel#gameIcon { background:%4; border:1px solid %1; border-radius:5px; color:palette(placeholder-text); font-size:10px; }
        QLabel#gameStatus { color:palette(placeholder-text); }
        QLabel#gameStatus[running="true"] { color:%5; }
        QPushButton#more { padding:0; color:palette(placeholder-text); }
        QWidget#otherHeader { background:transparent; }
        QWidget#otherHeader[interactive="true"]:hover { background:%2; }
        QWidget#otherHeader[hasRunning="true"] { border-top:1px dotted %1; }
        QPushButton#otherGames { text-align:left; border:0; border-radius:0; padding:10px 0; background:transparent; color:palette(placeholder-text); }
        QPushButton#otherGames:hover { background:transparent; }
        QLabel#installedGamesCount { border:1px solid %1; border-radius:5px; padding:1px 6px; color:palette(placeholder-text); }
        QPushButton#installedGamesMore { padding:0; }
        QPushButton#flushDetailsToggle { text-align:left; border:0; border-radius:0; padding:4px 0; background:transparent; color:palette(placeholder-text); }
        QPushButton#flushDetailsToggle:hover { color:palette(text); }
        QPushButton#flushDetailsToggle:focus { border:0; }
        QPushButton#load { border-top-right-radius:0; border-bottom-right-radius:0; border-right-width:0; }
        QPushButton#load:disabled { background:transparent; border-color:%1; }
        QPushButton#save, QPushButton#load { font-size:15px; }
        QPushButton#historyArrow { border-top-left-radius:0; border-bottom-left-radius:0; border-left-width:1px; padding:0; }
        QPushButton#historyArrow:disabled { background:transparent; border-color:%1; }
        QProgressBar { border:0; background:%1; }
        QProgressBar::chunk { background:#9747ff; }
        QScrollArea { border:0; background:transparent; }
        QLabel#emptyState { color:palette(placeholder-text); }
        QLabel#keycap { background:%4; border-radius:3px; padding:2px 4px; color:palette(placeholder-text); }
        QLabel#error { color:%6; }
        QMenu, QWidget#historyPopup { background:palette(window); border:1px solid %1; }
        QMenu#iconMenu { padding:0; }
        QPushButton#iconMenuItem { background:transparent; border:0; border-radius:0; padding:0; text-align:left; }
        QPushButton#iconMenuItem:hover { background:%4; }
        QPushButton#iconMenuItem:disabled { background:transparent; }
        QLabel#iconMenuIcon { background:transparent; border:0; }
        QFrame#iconMenuSeparator { color:%1; margin:3px 8px; }
        QWidget[historyRow="true"] { border-radius:0; }
        QWidget[historyRow="true"]:hover { background:%4; }
        QFrame#historyDateDivider { color:%1; }
        QPushButton#historyAction, QPushButton#historyDelete { color:palette(placeholder-text); padding:3px; }
        QPushButton#historyAction:disabled, QPushButton#historyDelete:disabled { color:palette(disabled,button-text); }
        QLabel#day { color:palette(placeholder-text); font-weight:600; padding-top:6px; }
        QLineEdit { padding:5px; border:1px solid %1; background:palette(base); }
        QCheckBox { spacing:6px; }
        QCheckBox::indicator { width:12px; height:12px; border:1px solid %1; border-radius:2px; }
        QCheckBox::indicator:checked { image:url(:/icons/check.svg); background:#9747ff; border:1px solid #9747ff; }
    )")
                            .arg(dark ? "#3b3b3b" : "#cecece", dark ? "#1f1f1f" : "#eaeaea",
                                 dark ? "#101010" : "#dedede", dark ? "#303030" : "#d8d8d8",
                                 dark ? "#75e8b0" : "#167747", dark ? "#ffb2a9" : "#9d2525",
                                 dark ? "#bc8dff" : "#7b2ed7"));
}
MainWindow::MainWindow(Service *service, bool demo, QWidget *parent)
    : QMainWindow(parent), service_(service) {
    if (!qApp->findChild<QObject *>("disabledButtonCursorFilter")) {
        auto *filter = new DisabledButtonCursorFilter(qApp);
        filter->setObjectName("disabledButtonCursorFilter");
        qApp->installEventFilter(filter);
    }
    setWindowTitle(demo ? "SaveScummer — Demo" : "SaveScummer");
    setWindowIcon(QIcon(":/icon.svg"));
    setMinimumWidth(340);
    resize(620, 420);
    auto *central = new QWidget;
    auto *layout = new QVBoxLayout(central);
    layout->setContentsMargins(0, 0, 0, 0);
    layout->setSpacing(0);
    auto *scroll = new QScrollArea;
    scroll_ = scroll;
    scroll->setWidgetResizable(true);
    scroll->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
    games_ = new QWidget;
    gamesLayout_ = new QVBoxLayout(games_);
    gamesLayout_->setContentsMargins(0, 0, 0, 0);
    gamesLayout_->setSpacing(0);
    gamesLayout_->setAlignment(Qt::AlignTop);
    empty_ = label("No installed games. Scan for known games or add a custom game.", games_);
    empty_->setObjectName("emptyState");
    empty_->setWordWrap(true);
    empty_->setContentsMargins(contentInsets(width()));
    otherHeader_ = new QWidget(games_);
    otherHeader_->setObjectName("otherHeader");
    otherHeader_->setAttribute(Qt::WA_StyledBackground);
    otherHeaderLayout_ = new QGridLayout(otherHeader_);
    const auto headerInsets = contentInsets(width());
    otherHeaderLayout_->setContentsMargins(headerInsets.left(), 0, headerInsets.right(), 0);
    otherHeaderLayout_->setVerticalSpacing(6);
    otherHeaderLayout_->setHorizontalSpacing(8);
    otherToggle_ = new QPushButton(otherHeader_);
    otherToggle_->setObjectName("otherGames");
    otherToggle_->setCheckable(true);
    otherToggle_->setSizePolicy(QSizePolicy::Maximum, QSizePolicy::Preferred);
    connect(otherToggle_, &QPushButton::clicked, this, [this] { setOtherGamesOpen(!othersOpen_); });
    otherCount_ = label("0", otherHeader_);
    otherCount_->setObjectName("installedGamesCount");
    otherCount_->setAlignment(Qt::AlignCenter);
    otherCount_->setSizePolicy(QSizePolicy::Fixed, QSizePolicy::Fixed);
    otherActions_ = new QWidget(otherHeader_);
    auto *otherActionsLayout = new QHBoxLayout(otherActions_);
    otherActionsLayout->setContentsMargins(0, 0, 0, 0);
    otherActionsLayout->setSpacing(6);
    otherActionsLayout->addStretch();
    scanGames_ = new QPushButton("Scan for known games", otherActions_);
    scanGames_->setObjectName("scanGames");
    scanGames_->setIcon(paletteIcon(":/icons/search.svg"));
    // Qt can report a taller size hint for the Unicode scanning caption. Measure
    // every state before display so neither the button nor its focus rectangle moves.
    scanGames_->ensurePolished();
    const auto idleScanCaption = scanGames_->text();
    const int idleScanWidth = scanGames_->sizeHint().width();
    int scanButtonHeight = scanGames_->sizeHint().height();
    for (const auto &caption : {QString("Scanning…"), QString("No games found"),
                                QString("1 game found"), QString("99 games found")}) {
        scanGames_->setText(caption);
        scanButtonHeight = qMax(scanButtonHeight, scanGames_->sizeHint().height());
    }
    scanGames_->setText(idleScanCaption);
    scanGames_->setMinimumWidth(idleScanWidth);
    scanGames_->setFixedHeight(scanButtonHeight);
    scanResultTimer_ = new QTimer(this);
    scanResultTimer_->setObjectName("scanResultTimer");
    scanResultTimer_->setSingleShot(true);
    scanResultTimer_->setInterval(3000);
    connect(scanResultTimer_, &QTimer::timeout, this, [this] {
        scanResultText_.clear();
        refresh();
    });
    addGame_ = new SquareButton(otherActions_);
    addGame_->setObjectName("installedGamesMore");
    addGame_->setIcon(paletteIcon(":/icons/ellipsis.svg"));
    addGame_->setAccessibleName("Installed games options");
    addGame_->setToolTip("Installed games options");
    otherActionsLayout->addWidget(scanGames_);
    otherActionsLayout->addWidget(addGame_);
    const int installedActionHeight = scanButtonHeight;
    addGame_->setFixedSize(installedActionHeight, installedActionHeight);
    connect(scanGames_, &QPushButton::clicked, this, &MainWindow::scanForGames);
    connect(addGame_, &QPushButton::clicked, this, [this] {
        closePopup();
        auto *menu = new IconMenu(this);
        popup_ = menu;
        menu->addIconAction(paletteIcon(":/icons/add.svg"), "Add custom game",
                            [this] { addCustomGame(); });
        placePopup(menu, addGame_);
    });
    for (auto *clickable : {otherHeader_, otherActions_, static_cast<QWidget *>(otherCount_)}) {
        clickable->installEventFilter(this);
        clickable->setCursor(Qt::PointingHandCursor);
    }
    arrangeOtherHeader();
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
    for (const bool load : {false, true}) {
        auto *shortcut = new QShortcut(QKeySequence(load ? "Ctrl+F9" : "Ctrl+F5"), this);
        shortcut->setAutoRepeat(false);
        connect(shortcut, &QShortcut::activated, this, [this, load] { triggerSelectedShortcut(load); });
    }
#ifdef Q_OS_WIN
    // The host owns the global hotkeys. Its WM_HOTKEY handler forwards them to
    // this window when it (or an owned dialog) has focus. Local QShortcuts also
    // work in dev/demo sessions with host integrations disabled.
    SetPropW(reinterpret_cast<HWND>(winId()), L"SaveScummer.ShortcutTarget.v1",
             reinterpret_cast<HANDLE>(1));
#endif
    auto *clock = new QTimer(this);
    clock->setInterval(10000);
    connect(clock, &QTimer::timeout, this, &MainWindow::refresh);
    clock->start();
}
void MainWindow::triggerSelectedShortcut(bool load) {
    if (!isActiveWindow() || QApplication::activeModalWidget() || QApplication::activePopupWidget())
        return;
    auto *row = rows_.value(selected_, nullptr);
    if (!row || !row->isVisible()) return;
    QPushButton *button = load ? row->load : row->save;
    if (button->isVisible() && button->isEnabled()) button->click();
}
void MainWindow::scanForGames() {
    if (!connected_ || scanPending_ || state_["scan_in_progress"].toBool())
        return;
    gamesAtScanStart_.clear();
    const auto games = state_["games"].toObject();
    for (auto it = games.begin(); it != games.end(); ++it) {
        const auto game = it.value().toObject();
        if (game["origin"] == "known" && game["installed"].toBool())
            gamesAtScanStart_.insert(it.key());
    }
    scanResultTimer_->stop();
    scanResultText_.clear();
    scanPending_ = true;
    refresh();
    service_->request({{"type", "rescan"}}, [this](const auto &reply) {
        if (reply["type"] == "error") {
            scanPending_ = false;
            QMessageBox::warning(this, "Scan for known games", errorText(reply));
            refresh();
            return;
        }
        if (reply["type"] == "state") {
            finishScan(reply["state"].toObject());
            return;
        }
        service_->request({{"type", "state"}}, [this](const auto &stateReply) {
            if (stateReply["type"] == "error") {
                scanPending_ = false;
                QMessageBox::warning(this, "Scan for known games", errorText(stateReply));
                refresh();
                return;
            }
            finishScan(stateReply["state"].toObject());
        });
    });
}
void MainWindow::finishScan(const QJsonObject &state) {
    int found = 0;
    const auto games = state["games"].toObject();
    for (auto it = games.begin(); it != games.end(); ++it) {
        const auto game = it.value().toObject();
        if (game["origin"] == "known" && game["installed"].toBool() &&
            !gamesAtScanStart_.contains(it.key()))
            ++found;
    }
    scanPending_ = false;
    scanResultText_ = found == 0 ? "No games found"
                      : found == 1 ? "1 game found"
                                   : QString("%1 games found").arg(found);
    scanResultTimer_->start();
    applyState(state);
    refresh();
}
#ifdef Q_OS_WIN
bool MainWindow::nativeEvent(const QByteArray &eventType, void *message, qintptr *result) {
    const auto *event = static_cast<MSG *>(message);
    static const UINT shortcutMessage = RegisterWindowMessageW(L"SaveScummer.DesktopShortcut.v1");
    if (shortcutMessage && event->message == shortcutMessage) {
        // A queued hotkey must not act after focus has moved to another app.
        if (GetAncestor(GetForegroundWindow(), GA_ROOTOWNER) == reinterpret_cast<HWND>(winId()) &&
            (event->wParam == 1 || event->wParam == 2))
            triggerSelectedShortcut(event->wParam == 2);
        *result = 0;
        return true;
    }
    return QMainWindow::nativeEvent(eventType, message, result);
}
#endif
void MainWindow::resizeEvent(QResizeEvent *event) {
    QMainWindow::resizeEvent(event);
    if (event->size().width() != event->oldSize().width()) {
        const auto insets = contentInsets(event->size().width());
        empty_->setContentsMargins(insets);
        otherHeaderLayout_->setContentsMargins(insets.left(), 0, insets.right(), 0);
        arrangeOtherHeader();
        scheduleFitHeight();
    }
}
bool MainWindow::eventFilter(QObject *watched, QEvent *event) {
    if ((watched == otherHeader_ || watched == otherActions_ || watched == otherCount_) &&
        otherToggle_->isEnabled() &&
        event->type() == QEvent::MouseButtonRelease &&
        static_cast<QMouseEvent *>(event)->button() == Qt::LeftButton) {
        setOtherGamesOpen(!othersOpen_);
        return true;
    }
    return QMainWindow::eventFilter(watched, event);
}
void MainWindow::arrangeOtherHeader() {
    if (!otherHeaderLayout_)
        return;
    otherHeaderLayout_->removeWidget(otherToggle_);
    otherHeaderLayout_->removeWidget(otherCount_);
    otherHeaderLayout_->removeWidget(otherActions_);
    for (int col = 0; col < 4; ++col)
        otherHeaderLayout_->setColumnStretch(col, 0);
    otherHeaderLayout_->addWidget(otherToggle_, 0, 0);
    otherHeaderLayout_->addWidget(otherCount_, 0, 1);
    otherHeaderLayout_->setColumnStretch(2, 1);
    if (width() < 560) {
        otherHeaderLayout_->addWidget(otherActions_, 1, 0, 1, 4);
    } else {
        otherHeaderLayout_->addWidget(otherActions_, 0, 3);
    }
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
        const int desired = rowsHeight + footer_->sizeHint().height();
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
void MainWindow::setConnected(bool connected, const QString &) {
    if (!connected) { ++historyRequest_; historyLoading_ = false; historyRevision_ = -1; }
    if (connected && !connected_)
        resetRevision_ = true;
    connected_ = connected;
    sounds_->setEnabled(connected && !soundSettingPending_);
    startup_->setEnabled(connected && !startupSettingPending_);
    if (!connected)
        submitting_.clear();
    if (!connected)
        scanPending_ = false;
    refresh();
}
void MainWindow::applyState(const QJsonObject &state) {
    if (!resetRevision_ && state["revision"].toInteger() < state_["revision"].toInteger())
        return;
    resetRevision_ = false;
    state_ = state;
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
    QStringList other;
    for (const auto &id : order)
        if (!running.contains(id))
            other.append(id);
    if (!groupInitialized_) {
        othersOpen_ = running.isEmpty();
        groupInitialized_ = true;
    }
    if (!order.contains(selected_))
        selected_ = !running.isEmpty() ? running.first() : QString();
    if (!running.isEmpty() && !hadRunning_)
        othersOpen_ = !running.contains(selected_);
    if (!running.isEmpty() && !selected_.isEmpty() && !running.contains(selected_))
        othersOpen_ = true;
    if (running.isEmpty() && hadRunning_)
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
    otherHeader_->setVisible(true);
    const bool canToggleInstalled = !running.isEmpty();
    if (otherHeader_->property("hasRunning").toBool() != canToggleInstalled ||
        otherHeader_->property("interactive").toBool() != canToggleInstalled ||
        !otherHeader_->property("interactive").isValid()) {
        otherHeader_->setProperty("hasRunning", canToggleInstalled);
        otherHeader_->setProperty("interactive", canToggleInstalled);
        otherHeader_->style()->unpolish(otherHeader_);
        otherHeader_->style()->polish(otherHeader_);
    }
    otherToggle_->setText("Installed games");
    otherCount_->setText(QString::number(other.size()));
    otherToggle_->setIcon(paletteIcon(othersOpen_ ? ":/icons/chevron-down.svg"
                                                  : ":/icons/chevron-right.svg"));
    otherToggle_->setChecked(othersOpen_);
    otherToggle_->setEnabled(canToggleInstalled);
    const auto installedCursor = canToggleInstalled ? Qt::PointingHandCursor : Qt::ArrowCursor;
    otherHeader_->setCursor(installedCursor);
    otherActions_->setCursor(installedCursor);
    otherCount_->setCursor(installedCursor);
    otherToggle_->setCursor(installedCursor);
    otherToggle_->setAccessibleName(
        QString("Installed games, %1").arg(other.size()));
    const bool scanning = scanPending_ || state_["scan_in_progress"].toBool();
    scanGames_->setText(scanning          ? "Scanning…"
                        : !scanResultText_.isEmpty() ? scanResultText_
                                                     : "Scan for known games");
    // Keep a focused scan button enabled so Qt does not transfer focus to the
    // adjacent options button. scanForGames() rejects activation while busy.
    scanGames_->setEnabled(connected_);
    scanGames_->setAccessibleDescription(scanning ? "Scan in progress" : QString());
    addGame_->setEnabled(connected_);
    scanGames_->setCursor(connected_ && !scanning ? Qt::ArrowCursor : Qt::ForbiddenCursor);
    addGame_->setCursor(addGame_->isEnabled() ? Qt::ArrowCursor : Qt::ForbiddenCursor);
    bool inserted = false;
    for (const auto &id : order) {
        if (!running.contains(id) && !inserted) {
            gamesLayout_->addWidget(otherHeader_);
            empty_->setVisible(othersOpen_ && other.isEmpty());
            gamesLayout_->addWidget(empty_);
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
        row->setVisible(running.contains(id) || othersOpen_);
    }
    if (!inserted) {
        gamesLayout_->addWidget(otherHeader_);
        empty_->setVisible(othersOpen_ && other.isEmpty());
        gamesLayout_->addWidget(empty_);
    }
    if (historyBody_) {
        populateHistory();
        if (connected_ && !historyLoading_ && (historyRevision_ < 0 ||
            state_["history_status"].toObject()[historyGame_].toObject()["revision"].toInteger() > historyRevision_))
            fetchHistory(false, true);
    }
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
    if (state_["active_stack"].toArray().isEmpty())
        open = true;
    othersOpen_ = open;
    closePopup();
    if (!open && !state_["active_stack"].toArray().contains(selected_)) {
        const auto order = Presentation::orderedGames(state_);
        selected_.clear();
        for (const auto &value : state_["active_stack"].toArray())
            if (order.contains(value.toString())) {
                selected_ = value.toString();
                break;
            }
    }
    refresh();
}
void MainWindow::closePopup() {
    ++historyRequest_;
    historyRows_ = {};
    historyCursor_.clear();
    historyRevision_ = -1;
    historyLoading_ = false;
    historyScroll_.clear();
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
    int popupX = point.x();
    if (auto *menu = dynamic_cast<IconMenu *>(popup)) {
        popupX = anchor->mapToGlobal(QPoint(anchor->width() / 2, 0)).x() -
                 menu->iconColumnCenter();
    }
    popup->move(qBound(screen.left() + 6, popupX, screen.right() - popup->width() - 6),
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
    layout->setContentsMargins(0, 4, 0, 4);
    auto *scroll = new QScrollArea;
    scroll->setWidgetResizable(true);
    scroll->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
    historyBody_ = new QWidget;
    new QVBoxLayout(historyBody_);
    scroll->setWidget(historyBody_);
    historyScroll_ = scroll;
    layout->addWidget(scroll);
    const auto below = rows_[game]->arrow->mapToGlobal(QPoint(0, rows_[game]->arrow->height() + 3));
    const int availableHeight =
        rows_[game]->screen()->availableGeometry().bottom() - below.y() - 16;
    scroll->setFixedSize(qMin(390, screen()->availableGeometry().width() - 24),
                         qMin(350, qMax(60, availableHeight)));
    populateHistory();
    placePopup(popup, rows_[game]->load);
    connect(scroll->verticalScrollBar(),&QScrollBar::actionTriggered,this,[this,bar=QPointer<QScrollBar>(scroll->verticalScrollBar())](int) {
        QTimer::singleShot(0,this,[this,bar] {
            if (bar && bar->maximum()>0 && bar->value()==bar->maximum() && !historyCursor_.isEmpty()) fetchHistory(true);
        });
    });
    fetchHistory();
}
void MainWindow::fetchHistory(bool older, bool preserveAnchor) {
    if (!historyBody_ || historyLoading_ || !connected_) return;
    const auto game = historyGame_;
    const auto generation = ++historyRequest_;
    const QPointer<QWidget> popup = popup_;
    historyLoading_ = true;
    QJsonObject command{{"type", "history"}, {"game_id", game}, {"limit", 50}};
    if (older && !historyCursor_.isEmpty()) command["cursor"] = historyCursor_;
    QString anchor;
    if (preserveAnchor && historyScroll_) {
        for (const auto &value : historyRows_) {
            const auto id=value.toObject()["id"].toString();
            auto *widget=historyBody_->findChild<QWidget *>("historyRow-"+id);
            if (widget && widget->mapTo(historyScroll_->viewport(),QPoint(0,widget->height())).y()>0) { anchor=id; break; }
        }
        if (!anchor.isEmpty()) command["anchor_id"]=anchor;
    }
    service_->request(command, [this, game, generation, popup, older, anchor](const auto &reply) {
        if (generation != historyRequest_ || game != historyGame_ || !popup || popup != popup_ || !popup->isVisible()) return;
        historyLoading_ = false;
        if (reply["type"] == "error") {
            if (reply["error"].toObject()["code"] == "cursor_expired") {
                historyRevision_ = -1;
                fetchHistory(false, true);
            } else {
                historyRevision_ = state_["history_status"].toObject()[game].toObject()["revision"].toInteger();
                showError(game, errorText(reply));
            }
            return;
        }
        if (reply["type"] != "history_page") { showError(game, "Unexpected history response."); return; }
        const auto page = reply["page"].toObject();
        if (page["game_id"] != game) return;
        if (!older) historyRows_ = {};
        for (const auto &row : page["rows"].toArray()) historyRows_.append(row);
        while (historyRows_.size() > 200) historyRows_.removeFirst();
        historyRevision_ = page["revision"].toInteger();
        historyCursor_ = page["next_cursor"].toString();
        populateHistory();
        if (!older && historyScroll_ && !anchor.isEmpty())
            if (auto *widget = historyBody_->findChild<QWidget *>("historyRow-" + anchor))
                historyScroll_->ensureWidgetVisible(widget);
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
    layout->setContentsMargins(0, 4, 0, 4);
    layout->setSpacing(4);
    const bool enabled = connected_ && !submitting_.contains(historyGame_) &&
                         Presentation::blockingOperation(state_, historyGame_).isEmpty();
    QString previousDay;
    const QMap<QString, QString> names{{"saved", "Saved"},
                                       {"existing_backup", "Existing backup"},
                                       {"loaded", "Loaded"},
                                       {"reverted", "Reverted"},
                                       {"game_started", "Game started"},
                                       {"game_closed", "Game closed"}};
    const QMap<QString, QString> icons{{"saved", ":/icons/save.svg"},
                                       {"existing_backup", ":/icons/explore.svg"},
                                       {"loaded", ":/icons/load.svg"},
                                       {"reverted", ":/icons/restore.svg"},
                                       {"game_started", ":/icons/play.svg"},
                                       {"game_closed", ":/icons/stop.svg"}};
    for (const auto &value : historyRows_) {
        const auto row = value.toObject();
        const bool existing = row["kind"] == "existing_backup";
        const auto timestamp = row["display_time"];
        const auto date = timestamp.isDouble()
                              ? QDateTime::fromMSecsSinceEpoch(timestamp.toInteger()).toLocalTime()
                              : QDateTime();
        const auto group =
            existing ? QString("Other backups") : Presentation::day(row["recorded_at"].toInteger());
        if (group != previousDay) {
            auto *day = label(group);
            day->setObjectName("day");
            auto *dayRow = new QWidget;
            auto *dayLayout = new QHBoxLayout(dayRow);
            dayLayout->setContentsMargins(12, 0, 18, 0);
            dayLayout->addWidget(day);
            layout->addWidget(dayRow);
            previousDay = group;
        }
        auto *widget = new QWidget;
        widget->setObjectName("historyRow-" + row["id"].toString());
        widget->setProperty("historyRow", true);
        widget->setAttribute(Qt::WA_Hover);
        auto *line = new QHBoxLayout(widget);
        line->setContentsMargins(12, 3, 18, 3);
        line->setSpacing(0);
        auto *time = label(date.isValid() ? Presentation::historyTime(timestamp.toInteger(),
                                                                       QDateTime::currentDateTime())
                                          : "—");
        time->setObjectName("historyTime");
        time->setFixedWidth(historyTimeWidth(time->font()));
        time->setAlignment(Qt::AlignLeft | Qt::AlignVCenter);
        if (date.isValid())
            time->setToolTip((existing ? QString("Folder modified: ") : QString()) +
                             date.toString("yyyy-MM-dd HH:mm:ss t"));
        line->addWidget(time);
        line->addSpacing(7);
        auto *dateDivider = new QFrame;
        dateDivider->setObjectName("historyDateDivider");
        dateDivider->setFrameShape(QFrame::VLine);
        dateDivider->setFrameShadow(QFrame::Plain);
        dateDivider->setFixedWidth(1);
        line->addWidget(dateDivider);
        line->addSpacing(10);
        QString caption = names.value(str(row, "kind"), str(row, "kind"));
        if (row["target_time"].isDouble())
            caption += " [" + QDateTime::fromMSecsSinceEpoch(row["target_time"].toInteger()).toLocalTime().toString("dd MMM, HH:mm:ss") + "]";
        const auto action = row["action"].toObject();
        const bool available = row["available"].toBool();
        if (existing)
            caption += date.isValid() ? "\nFolder modified" : "\nSave time unknown";
        if (!action.isEmpty() && !available)
            caption += "\nBackup unavailable";
        auto *entryIcon = label("");
        entryIcon->setObjectName("historyEntryIcon");
        entryIcon->setFixedSize(16, 22);
        entryIcon->setAlignment(Qt::AlignCenter);
        entryIcon->setPixmap(QIcon(icons.value(str(row, "kind"))).pixmap(16, 16));
        line->addWidget(entryIcon);
        line->addSpacing(4);
        auto *text = label(caption);
        text->setObjectName("historyEntryLabel");
        text->setWordWrap(true);
        line->addWidget(text, 1);
        if (!action.isEmpty()) {
            const bool restore = action["type"] == "load";
            const auto actionName = restore ? QString("Restore") : QString("Revert");
            auto *button = new QPushButton;
            button->setObjectName("historyAction");
            button->setProperty("checkpoint", action["target"]);
            button->setIcon(paletteIcon(":/icons/restore.svg"));
            button->setFixedSize(28, 28);
            button->setEnabled(enabled && available);
            button->setCursor(button->isEnabled() ? Qt::ArrowCursor : Qt::ForbiddenCursor);
            button->setToolTip(
                restore
                    ? "Load this saved reset point; current game data will be kept as an undo point."
                    : "Restore the game data from before this operation; current data will be kept as another undo point.");
            button->setAccessibleName(actionName + " " + caption + " " + time->text());
            const auto game = historyGame_;
            connect(button, &QPushButton::clicked, this,
                    [this, game, action] { execute(game, action); });
            line->addSpacing(8);
            line->addWidget(button);

            auto *remove = new QPushButton;
            remove->setObjectName("historyDelete");
            remove->setProperty("checkpoint", action["target"]);
            remove->setIcon(paletteIcon(":/icons/delete.svg"));
            remove->setFixedSize(button->size());
            remove->setEnabled(enabled && available);
            remove->setCursor(remove->isEnabled() ? Qt::ArrowCursor : Qt::ForbiddenCursor);
            remove->setToolTip(
                "Permanently delete this reset point; current game data will not be changed.");
            remove->setAccessibleName("Delete reset point " + caption + " " + time->text());
            const auto target = action["target"];
            connect(remove, &QPushButton::clicked, this, [this, game, target] {
                execute(game, {{"type", "delete"}, {"target", target}});
            });
            line->addSpacing(8);
            line->addWidget(remove);
        }
        layout->addWidget(widget);
    }
    if (!historyCursor_.isEmpty()) {
        auto *older = new QPushButton("Load older");
        older->setObjectName("historyOlder");
        older->setIcon(paletteIcon(":/icons/history.svg"));
        older->setEnabled(connected_ && !historyLoading_);
        older->setCursor(older->isEnabled() ? Qt::ArrowCursor : Qt::ForbiddenCursor);
        connect(older, &QPushButton::clicked, this, [this] { fetchHistory(true); });
        layout->addWidget(older);
    }
    layout->addStretch();
}
void MainWindow::options(const QString &game) {
    closePopup();
    auto *menu = new IconMenu(this);
    popup_ = menu;
    menu->setProperty("game", game);
    const bool idle = connected_ && !submitting_.contains(game) &&
                      Presentation::blockingOperation(state_, game).isEmpty();
    menu->addIconAction(paletteIcon(":/icons/explore.svg"), "Open in File Explorer", [this, game] {
        service_->request({{"type", "explore"}, {"game_id", game}}, [this, game](const auto &reply) {
            if (reply["type"] == "error") showError(game, errorText(reply));
        });
    });
    auto *configureAction = menu->addIconAction(paletteIcon(":/icons/configure.svg"), "Configure…",
                                                [this, game] { configure(game); });
    configureAction->setEnabled(idle);
    configureAction->setProperty("requiresIdle", true);
    configureAction->setProperty("available", true);
    const bool any = state_["history_status"].toObject()[game].toObject()["can_flush"].toBool();
    auto *flushAction = menu->addIconAction(paletteIcon(":/icons/flush.svg"), "Flush history…",
                                            [this, game] { flush(game); });
    flushAction->setEnabled(idle && any);
    flushAction->setProperty("requiresIdle", true);
    flushAction->setProperty("available", any);
    if (gameOf(state_, game)["origin"] == "custom") {
        menu->addIconSeparator();
        auto *forgetAction = menu->addIconAction(paletteIcon(":/icons/delete.svg"), "Forget this game",
                                                 [this, game] { flush(game, true); });
        forgetAction->setEnabled(idle);
        forgetAction->setProperty("requiresIdle", true);
        forgetAction->setProperty("available", true);
    }
    placePopup(menu, rows_[game]->more);
}
void MainWindow::addCustomGame() {
    closePopup();
    auto *dialog = new QDialog(this);
    dialog->setObjectName("addCustomGameDialog");
    dialog->setAttribute(Qt::WA_DeleteOnClose);
    dialog->setWindowTitle("Add custom game");
    auto *form = new QFormLayout(dialog);
    auto *name = new QLineEdit;
    name->setObjectName("customGameName");
    auto *executable = new QLineEdit;
    executable->setObjectName("customGameExecutable");
    auto *directory = new QLineEdit;
    directory->setObjectName("customGameSaveLocation");
    auto addPath = [this, dialog, form, name](const QString &caption, QLineEdit *edit, bool folder) {
        auto *row = new QWidget;
        auto *layout = new QHBoxLayout(row);
        layout->setContentsMargins(0, 0, 0, 0);
        layout->addWidget(edit);
        auto *browse = new QPushButton("Browse…");
        browse->setObjectName(folder ? "customGameSaveLocationBrowse"
                                     : "customGameExecutableBrowse");
        browse->setIcon(paletteIcon(":/icons/explore.svg"));
        layout->addWidget(browse);
        QObject::connect(browse, &QPushButton::clicked, dialog, [this, dialog, edit, folder, name] {
            const auto path = folder
                                  ? QFileDialog::getExistingDirectory(dialog, "Save location",
                                                                      edit->text())
                                  : QFileDialog::getOpenFileName(dialog, "Game executable",
                                                                 edit->text());
            if (!path.isEmpty()) {
                edit->setText(QDir::toNativeSeparators(path));
                if (!folder && name->text().trimmed().isEmpty())
                    name->setText(QFileInfo(path).completeBaseName());
            }
        });
        form->addRow(caption, row);
    };
    addPath("Game executable:", executable, false);
    addPath("Save location:", directory, true);
    form->addRow("Name:", name);
    auto *error = label("");
    error->setObjectName("error");
    error->setWordWrap(true);
    form->addRow(error);
    auto *buttons = new QDialogButtonBox(QDialogButtonBox::Ok | QDialogButtonBox::Cancel);
    auto *add = buttons->button(QDialogButtonBox::Ok);
    add->setObjectName("addCustomGameSubmit");
    add->setText("Add");
    form->addRow(buttons);
    connect(buttons, &QDialogButtonBox::rejected, dialog, &QDialog::reject);
    const QPointer<QDialog> guard(dialog);
    connect(buttons, &QDialogButtonBox::accepted, dialog,
            [this, guard, name, executable, directory, error, add] {
                if (!guard)
                    return;
                if (name->text().trimmed().isEmpty() || executable->text().trimmed().isEmpty() ||
                    directory->text().trimmed().isEmpty()) {
                    error->setText("Name, executable and save location are required.");
                    return;
                }
                guard->setProperty("submitting", true);
                add->setEnabled(false);
                service_->request({{"type", "add_custom_game"},
                                   {"name", name->text().trimmed()},
                                   {"executable", executable->text()},
                                   {"data_dir", directory->text()}},
                                  [this, guard, error, add](const auto &reply) {
                                      if (!guard)
                                          return;
                                      guard->setProperty("submitting", false);
                                      if (reply["type"] != "configured") {
                                          error->setText(errorText(reply));
                                          add->setEnabled(true);
                                          return;
                                      }
                                      const auto id = reply["game"].toObject()["id"].toString();
                                      guard->accept();
                                      othersOpen_ = true;
                                      service_->request({{"type", "state"}}, [this, id](const auto &state) {
                                          if (state["type"] == "state") {
                                              applyState(state["state"].toObject());
                                              selectGame(id);
                                          }
                                      });
                                  });
            });
    executable->setMinimumWidth(360);
    directory->setMinimumWidth(360);
    form->setSizeConstraint(QLayout::SetFixedSize);
    dialog->setSizeGripEnabled(false);
    dialog->setWindowFlag(Qt::MSWindowsFixedSizeDialogHint, true);
    dialog->open();
    executable->setFocus();
    setDialogDefault(buttons, add);
}
void MainWindow::configure(const QString &id) {
    closePopup();
    const auto game = gameOf(state_, id);
    auto *dialog = new QDialog(this);
    dialog->setObjectName("configureDialog");
    dialog->setAttribute(Qt::WA_DeleteOnClose);
    dialog->setWindowTitle("Configure " + str(game, "name"));
    dialog->setProperty("configuredGame", id);
    auto *form = new QFormLayout(dialog);
    auto *name = new QLineEdit(str(game, "name"));
    const bool custom = game["origin"] == "custom";
    if (custom)
        form->addRow("Name:", name);
    else {
        name->setParent(dialog);
        name->hide();
    }
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
    else {
        choices->setParent(dialog);
        choices->hide();
    }
    connect(choices, &QComboBox::currentIndexChanged, dialog, [choices, executable, directory](int index) {
        if (index <= 0) return;
        const auto location = choices->currentData().toJsonObject();
        executable->setText(location["executables"].toArray().first().toString());
        directory->setText(location["data_dir"].toString());
    });
    auto addPath = [this, dialog, form, choices, locations, custom](const QString &caption, QLineEdit *edit, bool folder) {
        auto *row = new QWidget;
        auto *layout = new QHBoxLayout(row);
        layout->setContentsMargins(0, 0, 0, 0);
        layout->addWidget(edit);
        auto *browse = new QPushButton("Browse…");
        auto *reset = new QPushButton("Reset");
        browse->setIcon(paletteIcon(":/icons/explore.svg"));
        reset->setIcon(paletteIcon(":/icons/reset.svg"));
        layout->addWidget(browse);
        if (!custom)
            layout->addWidget(reset);
        else {
            reset->setParent(dialog);
            reset->hide();
        }
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
    addPath(custom ? "Save location:" : "Game data dir (DIR):", directory, true);
    auto *error = label("");
    error->setObjectName("error");
    error->setWordWrap(true);
    form->addRow(error);
    auto *buttons = new QDialogButtonBox(QDialogButtonBox::Save | QDialogButtonBox::Cancel);
    form->addRow(buttons);
    auto *save = buttons->button(QDialogButtonBox::Save);
    save->setObjectName("configureSave");
    setDialogDefault(buttons, save);
    connect(buttons, &QDialogButtonBox::rejected, dialog, &QDialog::reject);
    const QPointer<QDialog> guard(dialog);
    connect(buttons, &QDialogButtonBox::accepted, dialog,
            [this, guard, game, id, name, directory, executable, error, buttons, choices] {
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
                                   {"name", name->text()},
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
    executable->setMinimumWidth(360);
    directory->setMinimumWidth(360);
    form->setSizeConstraint(QLayout::SetFixedSize);
    dialog->setSizeGripEnabled(false);
    dialog->setWindowFlag(Qt::MSWindowsFixedSizeDialogHint, true);
    dialog->open();
}
void MainWindow::flush(const QString &game, bool forget) {
    closePopup();
    service_->request({{"type", "flush_preview"}, {"game_id", game}}, [this, game, forget](const auto &reply) {
        if (reply["type"] != "flush_preview") {
            showError(game, errorText(reply));
            return;
        }
        const auto preview = reply["preview"].toObject();
        auto *dialog = new QDialog(this);
        dialog->setObjectName(forget ? "forgetDialog" : "flushDialog");
        dialog->setWindowTitle(forget ? "Forget this game" : "Flush history");
        dialog->setAttribute(Qt::WA_DeleteOnClose);
        dialog->setFixedWidth(520);
        auto *layout = new QGridLayout(dialog);
        layout->setContentsMargins(20, 20, 20, 16);
        layout->setHorizontalSpacing(16);
        layout->setVerticalSpacing(12);
        layout->setColumnStretch(1, 1);
        auto *warning = label("");
        warning->setPixmap(paletteIcon(":/icons/warning.svg").pixmap(40, 40));
        layout->addWidget(warning, 0, 0, 2, 1, Qt::AlignTop);
        auto *message = label(forget
                                  ? QString("Forget \"%1\"?")
                                        .arg(gameOf(state_, game)["name"].toString())
                                  : QString("Permanently delete all backups and clear this game's history?"));
        message->setWordWrap(true);
        layout->addWidget(message, 0, 1);
        auto *reassurance = label(
            forget ? "All backups, recovery points, incomplete copies, and history for this game "
                     "will be permanently deleted. The installed game, executable, and current "
                     "save data will be kept."
                   : "Your current game data will be kept.");
        reassurance->setWordWrap(true);
        layout->addWidget(reassurance, 1, 1);
        auto *toggle = new QPushButton(paletteIcon(":/icons/chevron-right.svg"), "Show details");
        toggle->setObjectName("flushDetailsToggle");
        toggle->setIconSize(QSize(20, 20));
        toggle->setCheckable(true);
        toggle->setAutoDefault(false);
        toggle->setCursor(Qt::PointingHandCursor);
        layout->addWidget(toggle, 2, 1, Qt::AlignLeft);
        QStringList paths;
        for (const auto &path : preview["paths"].toArray())
            paths.append(path.toString());
        QString details = QString("Saved backups: %1\nRecovery points: %2\nIncomplete copies: %3")
                              .arg(preview["saved"].toInteger())
                              .arg(preview["recovery"].toInteger())
                              .arg(preview["retained"].toInteger());
        if (!paths.isEmpty())
            details += "\n\n" + paths.join('\n');
        auto *detailsView = new QPlainTextEdit(details);
        detailsView->setObjectName("flushDetails");
        detailsView->setReadOnly(true);
        detailsView->setFixedHeight(160);
        detailsView->hide();
        layout->addWidget(detailsView, 3, 1);
        auto *buttons = new QDialogButtonBox(QDialogButtonBox::Yes | QDialogButtonBox::Cancel);
        buttons->button(QDialogButtonBox::Yes)->setText(forget ? "Forget game" : "Delete backups");
        setDialogDefault(buttons, buttons->button(QDialogButtonBox::Cancel));
        auto *nextPaths = new QPushButton("Next paths");
        nextPaths->setIcon(paletteIcon(":/icons/chevron-right.svg"));
        nextPaths->setAutoDefault(false);
        nextPaths->setObjectName("flushNextPaths");
        nextPaths->setProperty("cursor",preview["next_cursor"].toString());
        nextPaths->hide();
        layout->addWidget(nextPaths,4,1);
        connect(nextPaths,&QPushButton::clicked,dialog,[this,game,dialog=QPointer<QDialog>(dialog),detailsView,nextPaths,buttons,preview] {
            nextPaths->setEnabled(false);
            service_->request({{"type","flush_details"},{"game_id",game},{"cursor",nextPaths->property("cursor").toString()}},
                [dialog,detailsView,nextPaths,buttons,preview](const auto &reply) {
                    if (!dialog) return;
                    if (reply["type"] != "flush_preview" || reply["preview"].toObject()["revision"] != preview["revision"]) {
                        detailsView->setPlainText("Backups changed or details could not be loaded. Close this dialog and open Flush again.");
                        buttons->button(QDialogButtonBox::Yes)->setEnabled(false);
                        return;
                    }
                    const auto page = reply["preview"].toObject();
                    QStringList paths;
                    for (const auto &path : page["paths"].toArray()) paths.append(path.toString());
                    detailsView->setPlainText(QString("Saved backups: %1\nRecovery points: %2\nIncomplete copies: %3\n\n")
                        .arg(page["saved"].toInteger()).arg(page["recovery"].toInteger()).arg(page["retained"].toInteger())+paths.join('\n'));
                    nextPaths->setProperty("cursor",page["next_cursor"].toString());
                    nextPaths->setVisible(!page["next_cursor"].toString().isEmpty());
                    nextPaths->setEnabled(true);
                });
        });
        layout->addWidget(buttons, 5, 0, 1, 2);
        connect(toggle, &QPushButton::toggled, dialog, [dialog, toggle, detailsView,nextPaths](bool expanded) {
            toggle->setIcon(paletteIcon(expanded ? ":/icons/chevron-down.svg"
                                                  : ":/icons/chevron-right.svg"));
            toggle->setText(expanded ? "Hide details" : "Show details");
            detailsView->setVisible(expanded);
            nextPaths->setVisible(expanded && !nextPaths->property("cursor").toString().isEmpty());
            dialog->adjustSize();
        });
        connect(buttons->button(QDialogButtonBox::Yes), &QPushButton::clicked, dialog, &QDialog::accept);
        connect(buttons, &QDialogButtonBox::rejected, dialog, &QDialog::reject);
        connect(dialog, &QDialog::accepted, this, [this, game, preview, forget] {
            execute(game, {{"type", forget ? "forget" : "flush"},
                           {"confirmed_revision", preview["revision"]}});
        });
        dialog->open();
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
    box->setIconPixmap(paletteIcon(":/icons/warning.svg").pixmap(40, 40));
    box->setInformativeText(op["error"].toObject()["message"].toString());
    box->setDetailedText("Current data: " + str(op, "live") + "\nRecovery: " + str(op, "recovery") +
                         "\nOriginal: " + str(op, "original") + "\nStaging: " + str(op, "staging"));
    const QMap<QString, QString> choices{{"Keep current data", "keep_current"},
                                         {"Restore before operation", "restore_before"},
                                         {"Retry recovery", "retry"}};
    for (auto it = choices.begin(); it != choices.end(); ++it) {
        auto *button = box->addButton(it.key(), QMessageBox::ActionRole);
        const auto choice = it.value();
        button->setIcon(paletteIcon(choice == "retry"            ? ":/icons/reset.svg"
                                    : choice == "restore_before" ? ":/icons/restore.svg"
                                                                 : ":/icons/save.svg"));
        connect(button, &QPushButton::clicked, this, [this, game, op, choice] {
            execute(game, {{"type", "recover"}, {"operation", op["id"]}, {"choice", choice}});
        });
    }
    box->setAttribute(Qt::WA_DeleteOnClose);
    box->open();
}
