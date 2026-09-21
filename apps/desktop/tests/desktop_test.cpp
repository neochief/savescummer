#include "demo.h"
#include "mainwindow.h"
#include "presentation.h"
#include <QApplication>
#include <QDialog>
#include <QDialogButtonBox>
#include <QEventLoop>
#include <QFile>
#include <QFontDatabase>
#include <QJsonDocument>
#include <QLocalServer>
#include <QLineEdit>
#include <QLocale>
#include <QMenu>
#include <QPlainTextEdit>
#include <QProcess>
#include <QRegularExpression>
#include <QScreen>
#include <QSignalSpy>
#include <QTemporaryDir>
#include <QTest>
#include <QTextDocument>
#include <QWidgetAction>
#include <algorithm>
#include <QUuid>
#include <QtEndian>
#ifdef Q_OS_WIN
#define NOMINMAX
#include <windows.h>
#endif

class FakeService : public Service {
  public:
    using Service::Service;
    QJsonObject state = demoState();
    QList<QJsonObject> requests;
    bool delayHistory = false;
    QList<Callback> historyCallbacks;
    QJsonObject executeReply{
        {"type", "error"}, {"error", QJsonObject{{"code", "busy"}, {"message", "Game is busy."}}}};
    void start() override {
        emit connectionChanged(true, {});
        emit stateChanged(demoSummary(state));
    }
    void request(const QJsonObject &command, Callback callback = {}) override {
        requests.append(command);
        if (command["type"] == "history") {
            if (delayHistory) historyCallbacks.append(callback);
            else if (callback) callback(demoHistory(state,command));
            return;
        }
        if (command["type"] == "set_play_sounds") {
            state["settings"] = QJsonObject{{"play_sounds", command["enabled"]}};
            publish();
            if (callback)
                callback({{"type", "ok"}});
            return;
        }
        if (callback)
            callback(command["type"] == "execute"
                         ? executeReply
                         : QJsonObject{{"type", "state"}, {"state", demoSummary(state)}});
    }
    void publish() { state["revision"] = state["revision"].toInteger()+1; emit stateChanged(demoSummary(state)); }
};
class DesktopTest : public QObject {
    Q_OBJECT
  private slots:
    void initTestCase() {
#ifdef Q_OS_WIN
        QFontDatabase::addApplicationFont("C:/Windows/Fonts/segoeui.ttf");
        QFontDatabase::addApplicationFont("C:/Windows/Fonts/segoeuib.ttf");
        qApp->setFont(QFont("Segoe UI", 9));
#endif
        qApp->setStyle("Fusion");
        MainWindow::applyTheme(true);
    }
    void wireFixtures() {
        for (const auto &name : {"state", "save", "load", "revert", "delete", "sounds", "startup", "active", "explorer", "reset", "artwork", "history", "flush-details", "add-custom-game", "forget"}) {
            QFile file(QString(FIXTURE_DIR) + "/" + name + "-request.json");
            QVERIFY(file.open(QIODevice::ReadOnly));
            const auto fixture = QJsonDocument::fromJson(file.readAll()).object();
            QCOMPARE(
                Wire::envelope(fixture["command"].toObject(), fixture["request_id"].toString()),
                fixture);
            auto bytes = Wire::frame(fixture);
            QByteArray buffer = bytes.left(3);
            QVERIFY(Wire::consume(buffer).isEmpty());
            buffer += bytes.mid(3, 5);
            QVERIFY(Wire::consume(buffer).isEmpty());
            buffer += bytes.mid(8) + bytes;
            const auto decoded = Wire::consume(buffer);
            QCOMPARE(decoded.size(), 2);
            QCOMPARE(decoded[0], fixture);
            QVERIFY(buffer.isEmpty());
        }
        QByteArray invalid(4, '\0');
        QVERIFY_EXCEPTION_THROWN(Wire::consume(invalid), std::runtime_error);
        qToLittleEndian<quint32>(Wire::MaxFrame + 1, invalid.data());
        QVERIFY_EXCEPTION_THROWN(Wire::consume(invalid), std::runtime_error);
    }
    void soundPreferenceUsesHostStateAndCommand() {
        FakeService service;
        MainWindow window(&service);
        window.show();
        auto *sounds = window.findChild<QCheckBox *>("playSounds");
        QVERIFY(sounds);
        QVERIFY(!sounds->isEnabled());
        service.start();
        QVERIFY(sounds->isEnabled());
        QVERIFY(sounds->isChecked());
        QTest::mouseClick(sounds, Qt::LeftButton, Qt::NoModifier, QPoint(8, sounds->height() / 2));
        QVERIFY(!service.requests.isEmpty());
        QCOMPARE(service.requests.first(),
                 (QJsonObject{{"type", "set_play_sounds"}, {"enabled", false}}));
        QVERIFY(!sounds->isChecked());
        const auto count = service.requests.size();
        service.state["settings"] = QJsonObject{{"play_sounds", true}};
        service.publish();
        QVERIFY(sounds->isChecked());
        QCOMPARE(service.requests.size(), count);
        window.setConnected(false, "Disconnected");
        QVERIFY(!sounds->isEnabled());
        service.state["settings"] = QJsonObject{{"play_sounds", false}};
        service.start();
        QVERIFY(sounds->isEnabled());
        QVERIFY(!sounds->isChecked());
    }
    void plainInstructionsAndCheckpointIds() {
        const auto html = Presentation::instructions(
            "Procedure <script>:\n1. One & two\n2. Three\n\nLoad:\n1. Four");
        QCOMPARE(html.count("<ol "), 2);
        QVERIFY(!html.contains("<script>"));
        QTextDocument document;
        document.setHtml(html);
        QVERIFY(document.toPlainText().contains("<script>"));
        QCOMPARE(Presentation::historyAction({{"id", "history-id"},
                                              {"kind", "loaded"},
                                              {"recovery_id", "recovery-id"}})["target"]
                     .toString(),
                 QString("recovery-id"));
        QCOMPARE(Presentation::historyAction({{"id", "history-id"},
                                              {"kind", "saved"},
                                              {"snapshot_id", "checkpoint-id"}})["target"]
                     .toString(),
                 QString("checkpoint-id"));
        QVERIFY(Presentation::historyAction({{"kind", "game_started"}}).isEmpty());
        const QDateTime now(QDate(2026, 9, 21), QTime(18, 0, 0));
        QCOMPARE(Presentation::historyTime(now.addSecs(-3).toMSecsSinceEpoch(), now),
                 QString("3 seconds ago\n17:59:57"));
        QCOMPARE(Presentation::historyTime(
                     QDateTime(QDate(2026, 9, 21), QTime(12, 34, 56)).toMSecsSinceEpoch(), now),
                 QString("5 hours and 25 minutes ago\n12:34:56"));
        QCOMPARE(Presentation::historyTime(
                     QDateTime(QDate(2026, 9, 20), QTime(12, 34, 56)).toMSecsSinceEpoch(), now),
                 QString("Yesterday\n12:34:56"));
        QCOMPARE(Presentation::historyTime(
                     QDateTime(QDate(2026, 9, 18), QTime(12, 34, 56)).toMSecsSinceEpoch(), now),
                 QLocale().toString(QDate(2026, 9, 18), "dddd") + "\n12:34:56");
        QCOMPARE(Presentation::historyTime(
                     QDateTime(QDate(2024, 2, 3), QTime(4, 5, 6)).toMSecsSinceEpoch(), now),
                 QString("2024-02-03\n04:05:06"));
    }
    void focusedShortcuts_data() {
        QTest::addColumn<bool>("load");
        QTest::addColumn<bool>("gameRunning");
        QTest::addColumn<bool>("forwarded");
        for (bool load : {false, true})
            for (bool running : {false, true})
                for (bool forwarded : {false, true}) {
                    const auto name = QString("%1-%2-%3").arg(load ? "load" : "save")
                        .arg(running ? "other-game-running" : "no-game-running")
                        .arg(forwarded ? "host-forwarded" : "local");
                    QTest::newRow(qPrintable(name)) << load << running << forwarded;
                }
    }
    void focusedShortcuts() {
        QFETCH(bool, load);
        QFETCH(bool, gameRunning);
        QFETCH(bool, forwarded);
        if (forwarded && QGuiApplication::platformName() != "windows")
            QSKIP("Native host forwarding is tested with the Windows platform.");
        class PendingService : public FakeService {
          public:
            Callback pending;
            void request(const QJsonObject &command, Callback callback = {}) override {
                if (command["type"] == "execute") {
                    requests.append(command);
                    pending = std::move(callback);
                } else FakeService::request(command, std::move(callback));
            }
        } service;
        service.state["active_stack"] = gameRunning ? QJsonArray{"void-war"} : QJsonArray();
        MainWindow window(&service, true);
        window.show();
        service.start();
        window.setOtherGamesOpen(true);
        window.selectGame("ftl");
        window.activateWindow();
        QTRY_VERIFY(window.isActiveWindow());
        GameRow *selected = nullptr, *other = nullptr;
        for (auto *row : window.findChildren<GameRow *>()) {
            if (row->id == "ftl") selected = row;
            if (row->id == "void-war") other = row;
        }
        QVERIFY(selected && other);
        selected->header->setFocus();
        QTRY_COMPARE(QGuiApplication::applicationState(), Qt::ApplicationActive);
        QTest::qWait(30); // Let native activation/shortcut context events settle.
        auto pressShortcut = [&] {
#ifdef Q_OS_WIN
            if (forwarded) {
                const auto handle = reinterpret_cast<HWND>(window.winId());
                QCOMPARE(reinterpret_cast<quintptr>(GetPropW(handle, L"SaveScummer.ShortcutTarget.v1")),
                         quintptr(1));
                SendMessageW(handle, RegisterWindowMessageW(L"SaveScummer.DesktopShortcut.v1"),
                             load ? 2 : 1, 0);
                return;
            }
#endif
            QTest::keyClick(QApplication::focusWidget(), load ? Qt::Key_F9 : Qt::Key_F5,
                            Qt::ControlModifier);
        };
        pressShortcut();
        QCOMPARE(service.requests.size(), 1);
        QCOMPARE(service.requests.first()["type"].toString(), QString("execute"));
        QCOMPARE(service.requests.first()["game_id"].toString(), QString("ftl"));
        QCOMPARE(service.requests.first()["action"].toObject()["type"].toString(),
                 load ? QString("load") : QString("save"));
        QVERIFY(selected->progress->isVisible());
        QVERIFY(!other->progress->isVisible());
        QVERIFY(!selected->save->isEnabled());
        QVERIFY(!selected->load->isEnabled());
        pressShortcut();
        QCOMPARE(service.requests.size(), 1); // Busy/submitting must not send twice.

        auto operation = QJsonObject{{"id", "op"}, {"game_id", "ftl"}, {"status", "pending"},
                                    {"action", QJsonObject{{"type", load ? "load" : "save"}}}};
        service.state["operations"] = QJsonObject{{"op", operation}};
        auto reply = std::move(service.pending);
        reply({{"type", "accepted"}, {"operation_id", "op"}});
        QVERIFY(selected->progress->isVisible());
        operation["status"] = "completed";
        service.state["operations"] = QJsonObject{{"op", operation}};
        service.publish();
        QVERIFY(!selected->progress->isVisible());
        QVERIFY(selected->save->isEnabled());
        QVERIFY(selected->load->isEnabled());
        const auto count = service.requests.size();

        window.setConnected(false, "Disconnected");
        pressShortcut();
        QCOMPARE(service.requests.size(), count);
        window.setConnected(true);
        QDialog dialog(&window);
        dialog.setWindowModality(Qt::ApplicationModal);
        dialog.show();
        dialog.activateWindow();
        QTRY_VERIFY(dialog.isActiveWindow());
        pressShortcut();
        QCOMPARE(service.requests.size(), count);
        dialog.close();
        window.activateWindow();
        QTRY_VERIFY(window.isActiveWindow());
        window.selectGame("into-the-breach"); // Installed, but no save directory yet.
        pressShortcut();
        QCOMPARE(service.requests.size(), count);
        window.selectGame("ftl");
        QWidget otherWindow;
        otherWindow.show();
        otherWindow.activateWindow();
        QTRY_VERIFY(otherWindow.isActiveWindow());
        QTest::qWait(30);
        pressShortcut(); // A queued forwarded hotkey must not act after focus leaves.
        QCOMPARE(service.requests.size(), count);
    }
    void actionButtonsKeepTheirWidth_data() {
        QTest::addColumn<int>("windowWidth");
        QTest::addColumn<int>("pointSize");
        QTest::addColumn<bool>("dark");
        QTest::newRow("narrow-dark") << 340 << 9 << true;
        QTest::newRow("wide-light") << 650 << 9 << false;
        QTest::newRow("narrow-large-text") << 340 << 14 << true;
    }
    void gameAndEmptyStateUseConsistentInsets() {
        FakeService service;
        MainWindow window(&service, true);
        window.show();
        auto *empty = window.findChild<QLabel *>("emptyState");
        QVERIFY(empty);
        service.start();
        auto *row = window.findChild<GameRow *>("gameRow");
        QVERIFY(row);

        for (const int width : {620, 350}) {
            window.resize(width, 500);
            QCoreApplication::processEvents();
            const auto expected = width < 500 ? QMargins(14, 10, 14, 10)
                                              : QMargins(18, 12, 18, 12);
            QCOMPARE(empty->contentsMargins(), expected);
            QCOMPARE(row->layout()->contentsMargins(), expected);
        }
    }
    void actionButtonsKeepTheirWidth() {
        QFETCH(int, windowWidth);
        QFETCH(int, pointSize);
        QFETCH(bool, dark);
        MainWindow::applyTheme(dark);
        GameRow row("void-war");
        row.setFont(QFont("Segoe UI", pointSize));
        row.resize(windowWidth, 550);
        auto state = demoState();
        auto availability = state["availability"].toObject();
        auto available = availability["void-war"].toObject();
        available["default_snapshot_id"] = QJsonValue::Null;
        availability["void-war"] = available;
        state["availability"] = availability;
        row.updateState(state, true, true, false);
        for (QPushButton *button : {row.save, static_cast<QPushButton *>(row.load), row.arrow})
            button->setFont(QFont("Segoe UI", pointSize));
        row.updateState(state, true, true, false);
        row.show();
        QTest::qWait(30);
        QVERIFY(!row.save->icon().isNull());
        QVERIFY(!row.load->icon().isNull());
        QVERIFY(!row.arrow->icon().isNull());
        QVERIFY(!row.more->icon().isNull());
        QCOMPARE(row.load->font().pointSize(), pointSize);
        const int saveWidth = row.save->width();
        const int loadWidth = row.load->width();
        const int emptyHeight = row.load->height();
        QCOMPARE(row.load->accessibleName(), QString("Load, No checkpoints saved"));
        QCOMPARE(row.load->cursor().shape(), Qt::ForbiddenCursor);
        QCOMPARE(row.more->width(), row.arrow->width());
        const auto screenshotRoot = QString(SOURCE_DIR) + "/build/desktop/screenshots/";
        QDir().mkpath(screenshotRoot);
        QVERIFY(row.grab().save(screenshotRoot + QString("controls-%1-empty.png")
            .arg(QTest::currentDataTag())));
        QTest::mouseMove(row.info, row.info->rect().center());
        QCOMPARE(row.info->cursor().shape(), Qt::ArrowCursor);
        QCOMPARE(row.info->textInteractionFlags(), Qt::TextInteractionFlags(Qt::NoTextInteraction));

        auto snapshots = state["snapshots"].toObject();
        auto checkpoint = snapshots["void-war-saved"].toObject();
        available["default_snapshot_id"] = "void-war-saved";
        availability["void-war"] = available;
        state["availability"] = availability;
        // Existing folder timestamp -> unknown age -> newly saved checkpoint.
        // None of these state transitions may change the button widths.
        for (int step = 0; step < 3; ++step) {
            checkpoint["selection_time"] = step == 0
                ? QJsonValue(QDateTime(QDate(2020, 12, 31), QTime(23, 59, 59)).toMSecsSinceEpoch())
                : QJsonValue::Null;
            checkpoint["saved_at"] = step == 2
                ? QJsonValue(QDateTime::currentMSecsSinceEpoch()) : QJsonValue::Null;
            snapshots["void-war-saved"] = checkpoint;
            state["snapshots"] = snapshots;
            row.updateState(state, true, true, false);
            QCoreApplication::processEvents();
            QCOMPARE(row.save->width(), saveWidth);
            QCOMPARE(row.load->width(), loadWidth);
            QCOMPARE(row.save->width(), row.load->width() + row.arrow->width());
            QCOMPARE(row.save->height(), row.load->height());
            QVERIFY(row.more->geometry().right() < row.more->parentWidget()->width());
            QVERIFY(row.info->mapTo(&row, QPoint()).y() >=
                    row.save->mapTo(&row, QPoint(0, row.save->height())).y());
            QVERIFY(row.grab().save(screenshotRoot + QString("controls-%1-%2.png")
                .arg(QTest::currentDataTag()).arg(step)));
        }
        // Even an unusually long caption gets space instead of an ellipsis.
        row.load->setAge("Modified 23 hours and 59 minutes ago, additional checkpoint details", "Full timestamp");
        row.resize(windowWidth + 1, row.height());
        QTest::qWait(30);
        QVERIFY(row.load->height() > emptyHeight);
        QVERIFY(row.info->mapTo(&row, QPoint()).y() >=
                row.save->mapTo(&row, QPoint(0, row.save->height())).y());
        QVERIFY(row.grab().save(screenshotRoot + QString("controls-%1-long.png")
            .arg(QTest::currentDataTag())));
        MainWindow::applyTheme(true);
    }
    void flushDetailsDisclosure_data() {
        QTest::addColumn<QString>("finish");
        QTest::newRow("cancel") << "cancel";
        QTest::newRow("escape") << "escape";
        QTest::newRow("enter-defaults-to-cancel") << "enter";
        QTest::newRow("delete") << "delete";
    }
    void flushDetailsDisclosure() {
        QFETCH(QString, finish);
        class FlushService : public FakeService {
          public:
            void request(const QJsonObject &command, Callback callback = {}) override {
                if (command["type"] != "flush_preview") {
                    FakeService::request(command, callback);
                    return;
                }
                requests.append(command);
                callback({{"type", "flush_preview"},
                          {"preview", QJsonObject{{"saved", 2}, {"recovery", 1}, {"retained", 1},
                                                  {"revision", "preview-revision"},
                                                  {"paths", QJsonArray{"C:/Games/Void War/backup"}}}}});
            }
        } service;
        MainWindow window(&service, true);
        window.show();
        service.start();
        GameRow *row = nullptr;
        for (auto *candidate : window.findChildren<GameRow *>())
            if (candidate->id == "void-war") row = candidate;
        QVERIFY(row);
        QTest::mouseClick(row->more, Qt::LeftButton);
        auto *menu = window.findChild<QMenu *>();
        QVERIFY(menu);
        auto *flush = menu->actions().last();
        QVERIFY(flush->isEnabled());
        flush->trigger();
        auto *dialog = window.findChild<QDialog *>("flushDialog");
        QVERIFY(dialog);
        auto *toggle = dialog->findChild<QPushButton *>("flushDetailsToggle");
        auto *details = dialog->findChild<QPlainTextEdit *>("flushDetails");
        auto *buttons = dialog->findChild<QDialogButtonBox *>();
        QVERIFY(toggle && details && buttons);
        QTRY_VERIFY(dialog->isVisible());
        QVERIFY(!details->isVisible());
        QCOMPARE(toggle->iconSize(), QSize(20, 20));
        QVERIFY(buttons->button(QDialogButtonBox::Cancel)->isDefault());
        const auto requestCount = service.requests.size();
        const auto collapsedHeight = dialog->height();
        const auto toggleLeft = toggle->x();
        const auto screenshotRoot = QString(SOURCE_DIR) + "/build/desktop/screenshots/";
        QDir().mkpath(screenshotRoot);
        QVERIFY(dialog->grab().save(screenshotRoot + "flush-collapsed.png"));
        QTest::mouseClick(toggle, Qt::LeftButton);
        QTRY_VERIFY(details->isVisible());
        QVERIFY(dialog->height() > collapsedHeight);
        QCOMPARE(toggle->x(), toggleLeft);
        QVERIFY(details->toPlainText().contains("Incomplete copies: 1"));
        QVERIFY(details->toPlainText().contains("C:/Games/Void War/backup"));
        QCOMPARE(service.requests.size(), requestCount);
        QVERIFY(dialog->grab().save(screenshotRoot + "flush-expanded.png"));
        QTest::keyClick(toggle, Qt::Key_Space);
        QTRY_VERIFY(!details->isVisible());
        QCOMPARE(dialog->height(), collapsedHeight);
        QCOMPARE(service.requests.size(), requestCount);
        if (finish == "escape") QTest::keyClick(dialog, Qt::Key_Escape);
        else if (finish == "enter") QTest::keyClick(toggle, Qt::Key_Return);
        else QTest::mouseClick(buttons->button(finish == "delete" ? QDialogButtonBox::Yes
                                                                 : QDialogButtonBox::Cancel),
                               Qt::LeftButton);
        QVERIFY(!dialog->isVisible());
        if (finish == "delete") {
            QCOMPARE(service.requests.size(), requestCount + 1);
            QCOMPARE(service.requests.last()["action"].toObject(),
                     (QJsonObject{{"type", "flush"}, {"confirmed_revision", "preview-revision"}}));
        } else {
            QCOMPARE(service.requests.size(), requestCount);
        }
    }
    void selectionAndRegrouping() {
        FakeService service;
        MainWindow window(&service, true);
        window.show();
        service.start();
        QVERIFY(!(window.windowFlags() & Qt::FramelessWindowHint));
        QVERIFY(window.windowFlags() & Qt::WindowTitleHint);
        QTest::qWait(30);
        QCOMPARE(window.selectedGame(), QString("void-war"));
        auto *voidRow = window.findChildren<GameRow *>().first();
        for (auto *row : window.findChildren<GameRow *>())
            if (row->id == "void-war")
                voidRow = row;
        QVERIFY(voidRow->save->isVisible());
        window.setOtherGamesOpen(true);
        for (auto *row : window.findChildren<GameRow *>()) {
            if (row->id == "ftl") {
                QCOMPARE(row->cursor().shape(), Qt::PointingHandCursor);
                QTest::keyClick(row->header, Qt::Key_Return);
                QCOMPARE(window.selectedGame(), QString("ftl"));
            }
        }
        window.selectGame("slay-the-spire");
        QCOMPARE(window.selectedGame(), QString("slay-the-spire"));
        QVERIFY(!voidRow->save->isVisible());
        service.state["active_stack"] = QJsonArray{"slay-the-spire", "void-war"};
        service.publish();
        QCOMPARE(window.selectedGame(), QString("slay-the-spire"));
        service.state["active_stack"] = QJsonArray{"void-war"};
        service.publish();
        QCOMPARE(window.selectedGame(), QString("slay-the-spire"));
        window.setOtherGamesOpen(false);
        QCOMPARE(window.selectedGame(), QString("void-war"));
        window.setOtherGamesOpen(true);
        window.selectGame("into-the-breach");
        for (auto *row : window.findChildren<GameRow *>())
            if (row->id == "into-the-breach") {
                QVERIFY(!row->save->isEnabled());
                QVERIFY(!row->load->isEnabled());
                QVERIFY(!row->arrow->isEnabled());
                QCOMPARE(row->save->cursor().shape(), Qt::ForbiddenCursor);
                QCOMPARE(row->load->cursor().shape(), Qt::ForbiddenCursor);
                QCOMPARE(row->arrow->cursor().shape(), Qt::ForbiddenCursor);
                QVERIFY(row->more->isEnabled());
                QTest::keyClick(row->header, Qt::Key_Space);
                QCOMPARE(window.selectedGame(), row->id);
            }
    }
    void configureDialogIsCompactAndNonResizable() {
        FakeService service;
        MainWindow window(&service, true);
        window.show();
        service.start();
        GameRow *row = nullptr;
        for (auto *candidate : window.findChildren<GameRow *>())
            if (candidate->id == "void-war")
                row = candidate;
        QVERIFY(row);
        QTest::mouseClick(row->more, Qt::LeftButton);
        QMenu *menu = nullptr;
        for (auto *candidate : window.findChildren<QMenu *>())
            if (candidate->isVisible())
                menu = candidate;
        QVERIFY(menu);
        QAction *configure = nullptr;
        for (auto *action : menu->actions())
            if (action->text() == "Configure…")
                configure = action;
        QVERIFY(configure);
        configure->trigger();
        auto *dialog = window.findChild<QDialog *>("configureDialog");
        QVERIFY(dialog);
        QTRY_VERIFY(dialog->isVisible());
        QCOMPARE(dialog->minimumSize(), dialog->maximumSize());
        const auto fixedSize = dialog->size();
        dialog->resize(fixedSize + QSize(200, 200));
        QCoreApplication::processEvents();
        QCOMPARE(dialog->size(), fixedSize);
        for (auto *orphan : dialog->findChildren<QLineEdit *>(QString(), Qt::FindDirectChildrenOnly))
            QVERIFY(!orphan->isVisible());
        auto *save = dialog->findChild<QPushButton *>("configureSave");
        QVERIFY(save && save->isDefault());
        QVERIFY(save->icon().isNull());
        const auto screenshotRoot = QString(SOURCE_DIR) + "/build/desktop/screenshots/";
        QDir().mkpath(screenshotRoot);
        QVERIFY(dialog->grab().save(screenshotRoot + "configure-known.png"));
        dialog->reject();
    }
    void customLibraryControlsAndForget() {
        class CustomService : public FakeService {
          public:
            void request(const QJsonObject &command, Callback callback = {}) override {
                requests.append(command);
                if (command["type"] == "flush_preview") {
                    if (callback)
                        callback({{"type", "flush_preview"},
                                  {"preview",
                                   QJsonObject{{"revision", state["revision"]},
                                               {"saved", 0},
                                               {"recovery", 0},
                                               {"retained", 0},
                                               {"paths", QJsonArray()}}}});
                    return;
                }
                FakeService::request(command, callback);
            }
        } service;
        service.state["active_stack"] = QJsonArray();
        MainWindow window(&service, true);
        window.show();
        service.start();
        auto *header = window.findChild<QWidget *>("otherHeader");
        auto *toggle = window.findChild<QPushButton *>("otherGames");
        auto *count = window.findChild<QLabel *>("installedGamesCount");
        auto *scan = window.findChild<QPushButton *>("scanGames");
        auto *more = window.findChild<QPushButton *>("installedGamesMore");
        QVERIFY(header && header->isVisible());
        QVERIFY(toggle && count && scan && more);
        QCOMPARE(toggle->text(), QString("Installed games"));
        QCOMPARE(count->text(), QString("5"));
        QVERIFY(count->height() < toggle->height());
        QCOMPARE(window.selectedGame(), QString());
        QVERIFY(toggle->isChecked());
        QTest::mouseClick(header, Qt::LeftButton, Qt::NoModifier,
                          QPoint(header->width() / 2, 2));
        QVERIFY(toggle->isChecked());
        QVERIFY(!toggle->isEnabled());
        QCOMPARE(toggle->cursor().shape(), Qt::ArrowCursor);
        QCOMPARE(window.selectedGame(), QString());
        QTest::mouseClick(count, Qt::LeftButton);
        QVERIFY(toggle->isChecked());
        QCOMPARE(window.selectedGame(), QString());
        QTest::mouseClick(scan, Qt::LeftButton);
        QVERIFY(std::any_of(service.requests.begin(), service.requests.end(), [](const auto &request) {
            return request["type"] == "rescan";
        }));

        QTest::mouseClick(more, Qt::LeftButton);
        auto *libraryMenu = window.findChild<QMenu *>();
        QVERIFY(libraryMenu);
        const auto screenshotRoot = QString(SOURCE_DIR) + "/build/desktop/screenshots/";
        QDir().mkpath(screenshotRoot);
        QVERIFY(libraryMenu->grab().save(screenshotRoot + "installed-games-menu.png"));
        QCOMPARE(libraryMenu->objectName(), QString("iconMenu"));
        QCOMPARE(more->width(), more->height());
        QCOMPARE(more->height(), scan->height());
        auto *libraryWidgetAction = qobject_cast<QWidgetAction *>(libraryMenu->actions().first());
        QVERIFY(libraryWidgetAction && libraryWidgetAction->defaultWidget());
        auto *libraryIcon = libraryWidgetAction->defaultWidget()->findChild<QLabel *>("iconMenuIcon");
        QVERIFY(libraryIcon);
        QCOMPARE(libraryIcon->width(), libraryWidgetAction->defaultWidget()->height());
        QCOMPARE(libraryIcon->height(), libraryWidgetAction->defaultWidget()->height());
        QCOMPARE(libraryIcon->alignment(), Qt::Alignment(Qt::AlignCenter));
        QAction *add = nullptr;
        for (auto *action : libraryMenu->actions())
            if (action->text() == "Add custom game")
                add = action;
        QVERIFY(add);
        add->trigger();
        auto *addDialog = window.findChild<QDialog *>("addCustomGameDialog");
        QVERIFY(addDialog);
        QVERIFY(addDialog->findChild<QLineEdit *>("customGameName"));
        QVERIFY(addDialog->findChild<QLineEdit *>("customGameExecutable"));
        QVERIFY(addDialog->findChild<QLineEdit *>("customGameSaveLocation"));
        addDialog->reject();

        window.setOtherGamesOpen(true);
        window.selectGame("custom-demo");
        GameRow *custom = nullptr;
        for (auto *row : window.findChildren<GameRow *>())
            if (row->id == "custom-demo")
                custom = row;
        QVERIFY(custom && custom->isVisible());
        QCOMPARE(custom->findChild<QLabel *>("gameStatus")->text(), QString("Uninstalled"));
        QTest::mouseClick(custom->more, Qt::LeftButton);
        QMenu *menu = nullptr;
        for (auto *candidate : window.findChildren<QMenu *>())
            if (candidate->isVisible())
                menu = candidate;
        QVERIFY(menu);
        QVERIFY(menu->grab().save(screenshotRoot + "game-options-menu.png"));
        QCOMPARE(menu->objectName(), QString("iconMenu"));
        for (auto *action : menu->actions()) {
            if (action->text().isEmpty())
                continue;
            auto *widgetAction = qobject_cast<QWidgetAction *>(action);
            QVERIFY(widgetAction && widgetAction->defaultWidget());
            auto *icon = widgetAction->defaultWidget()->findChild<QLabel *>("iconMenuIcon");
            QVERIFY(icon);
            QCOMPARE(icon->width(), widgetAction->defaultWidget()->height());
            QCOMPARE(icon->height(), widgetAction->defaultWidget()->height());
        }
        QAction *configure = nullptr;
        for (auto *action : menu->actions())
            if (action->text() == "Configure…")
                configure = action;
        QVERIFY(configure);
        configure->trigger();
        auto *configureDialog = window.findChild<QDialog *>("configureDialog");
        QVERIFY(configureDialog);
        QTRY_VERIFY(configureDialog->isVisible());
        QCOMPARE(configureDialog->minimumSize(), configureDialog->maximumSize());
        auto *configureButtons = configureDialog->findChild<QDialogButtonBox *>();
        QVERIFY(configureButtons);
        auto *configureSave = configureButtons->button(QDialogButtonBox::Save);
        QVERIFY(configureSave->isDefault());
        QVERIFY(configureSave->icon().isNull());
        QVERIFY(configureDialog->grab().save(screenshotRoot + "configure.png"));
        configureDialog->reject();

        QTest::mouseClick(custom->more, Qt::LeftButton);
        menu = nullptr;
        for (auto *candidate : window.findChildren<QMenu *>())
            if (candidate->isVisible())
                menu = candidate;
        QVERIFY(menu);
        QAction *forget = nullptr;
        for (auto *action : menu->actions())
            if (action->text() == "Forget this game")
                forget = action;
        QVERIFY(forget && forget->isEnabled());
        forget->trigger();
        auto *dialog = window.findChild<QDialog *>("forgetDialog");
        QVERIFY(dialog);
        dialog->findChild<QDialogButtonBox *>()->button(QDialogButtonBox::Yes)->click();
        QVERIFY(std::any_of(service.requests.begin(), service.requests.end(), [](const auto &request) {
            return request["type"] == "execute" &&
                   request["action"].toObject()["type"] == "forget";
        }));
    }
    void busyRecoveryDisconnectAndFailure() {
        FakeService service;
        MainWindow window(&service, true);
        window.show();
        service.start();
        GameRow *row = nullptr;
        for (auto *r : window.findChildren<GameRow *>())
            if (r->id == "void-war")
                row = r;
        QVERIFY(row);
        QTest::mouseClick(row->save, Qt::LeftButton);
        QCOMPARE(service.requests.size(), 1);
        QCOMPARE(service.requests[0]["action"].toObject()["type"].toString(), QString("save"));
        QVERIFY(row->error->text().contains("busy"));
        service.state["operations"] = QJsonObject{{"op", QJsonObject{{"id", "op"},
                                                                     {"game_id", "void-war"},
                                                                     {"status", "pending"},
                                                                     {"bytes_copied", 512},
                                                                     {"phase", "preparing"}}}};
        service.publish();
        QVERIFY(!row->save->isEnabled());
        QVERIFY(!row->load->isEnabled());
        QVERIFY(!row->arrow->isEnabled());
        QVERIFY(row->progress->isVisible());
        window.setOtherGamesOpen(true);
        window.selectGame("ftl");
        window.selectGame("void-war");
        QVERIFY(row->progress->isVisible());
        service.state["operations"] = QJsonObject{
            {"op",
             QJsonObject{{"id", "op"}, {"game_id", "void-war"}, {"status", "recovery_needed"}}}};
        service.publish();
        QVERIFY(!row->save->isEnabled());
        QVERIFY(row->recovery->isVisible());
        QVERIFY(row->more->isEnabled());
        service.state["operations"] = QJsonObject();
        service.publish();
        window.setConnected(false, "Connection lost");
        QVERIFY(!row->save->isEnabled());
        QVERIFY(!row->load->isEnabled());
        window.setConnected(true);
        QVERIFY(row->save->isEnabled());
    }
    void historyAvailability() {
        FakeService service;
        MainWindow window(&service, true);
        window.show();
        service.start();
        GameRow *row = nullptr;
        for (auto *r : window.findChildren<GameRow *>())
            if (r->id == "void-war")
                row = r;
        auto snapshots = service.state["snapshots"].toObject();
        auto snapshot = snapshots["void-war-saved"].toObject();
        snapshot["available"] = false;
        snapshots["void-war-saved"] = snapshot;
        service.state["snapshots"] = snapshots;
        auto availability = service.state["availability"].toObject();
        auto item = availability["void-war"].toObject();
        item["default_snapshot_id"] = QJsonValue::Null;
        availability["void-war"] = item;
        service.state["availability"] = availability;
        service.publish();
        QVERIFY(!row->load->isEnabled());
        QVERIFY(row->arrow->isEnabled());
        QTest::mouseClick(row->arrow, Qt::LeftButton);
        auto *action = window.findChild<QPushButton *>("historyAction");
        auto *remove = window.findChild<QPushButton *>("historyDelete");
        QVERIFY(action);
        QVERIFY(remove);
        QVERIFY(!action->isEnabled());
        QVERIFY(!remove->isEnabled());
        snapshot["available"] = true;
        snapshots["void-war-saved"] = snapshot;
        service.state["snapshots"] = snapshots;
        service.publish();
        action = window.findChild<QPushButton *>("historyAction");
        remove = window.findChild<QPushButton *>("historyDelete");
        QVERIFY(action->isEnabled());
        QVERIFY(remove->isEnabled());
        QCOMPARE(action->size(), remove->size());
        QTest::mouseClick(action, Qt::LeftButton);
        QCOMPARE(service.requests.last()["action"].toObject()["target"].toString(),
                 QString("void-war-saved"));
    }
    void pagedHistoryBoundsWidgetsAndDiscardsStaleReplies() {
        FakeService service;
        QJsonArray history;
        for (int i=0;i<350;++i)
            history.append(QJsonObject{{"id",QString("row-%1").arg(i)},{"game_id","void-war"},
                {"kind","saved"},{"sequence",i+1},{"recorded_at",1000},{"snapshot_id","void-war-saved"}});
        service.state["history"]=history;
        service.state["visible_history"]=history;
        MainWindow window(&service,true);
        window.show(); service.start();
        GameRow *row=nullptr;
        for (auto *candidate:window.findChildren<GameRow *>()) if (candidate->id=="void-war") row=candidate;
        QVERIFY(row);
        QTest::mouseClick(row->arrow,Qt::LeftButton);
        QCOMPARE(window.findChildren<QPushButton *>("historyAction").size(),50);
        for (int i=0;i<6;++i) {
            auto *older=window.findChild<QPushButton *>("historyOlder"); QVERIFY(older); older->click();
            QVERIFY(window.findChildren<QPushButton *>("historyAction").size()<=200);
            QCoreApplication::sendPostedEvents(nullptr,QEvent::DeferredDelete);
        }
        QVERIFY(!window.findChild<QPushButton *>("historyOlder"));
        QVERIFY(!window.findChild<QPushButton *>("historyLatest"));
        QTest::mouseClick(row->arrow,Qt::LeftButton);
        QCoreApplication::sendPostedEvents(nullptr,QEvent::DeferredDelete);
        QTest::mouseClick(row->arrow,Qt::LeftButton);
        QCOMPARE(window.findChildren<QPushButton *>("historyAction").size(),50);
        QTest::mouseClick(row->arrow,Qt::LeftButton);
        service.delayHistory=true;
        QTest::mouseClick(row->arrow,Qt::LeftButton);
        QCOMPARE(service.historyCallbacks.size(),1);
        QTest::mouseClick(row->arrow,Qt::LeftButton);
        service.historyCallbacks.takeFirst()(demoHistory(service.state,{{"game_id","void-war"}}));
        QVERIFY(!window.findChild<QWidget *>("historyPopup") || !window.findChild<QWidget *>("historyPopup")->isVisible());
        QCoreApplication::sendPostedEvents(nullptr,QEvent::DeferredDelete);
        QTest::mouseClick(row->arrow,Qt::LeftButton);
        QCOMPARE(service.historyCallbacks.size(),1);
        window.setConnected(false);
        service.historyCallbacks.takeFirst()(demoHistory(service.state,{{"game_id","void-war"}}));
        QVERIFY(window.findChildren<QPushButton *>("historyAction").isEmpty());
    }
    void existingBackupsShowFolderTimes() {
        FakeService service;
        const auto modified = QDateTime(QDate(2026, 9, 18), QTime(12, 34, 56));
        const auto discovered = modified.addDays(2).toMSecsSinceEpoch();
        QJsonObject snapshots;
        QJsonArray history;
        for (int i = 0; i < 2; ++i) {
            const auto id = QString("copy-%1").arg(i);
            snapshots[id] = QJsonObject{{"saved_at", QJsonValue::Null},
                                        {"selection_time", modified.addDays(i).toMSecsSinceEpoch()},
                                        {"discovered_at", discovered},
                                        {"available", true}};
            history.append(QJsonObject{{"id", id + "-history"},
                                       {"game_id", "void-war"},
                                       {"kind", "existing_backup"},
                                       {"sequence", i + 1},
                                       {"recorded_at", discovered},
                                       {"snapshot_id", id}});
        }
        service.state["snapshots"] = snapshots;
        service.state["history"] = history;
        service.state["visible_history"] = history;
        auto availability = service.state["availability"].toObject();
        availability["void-war"] = QJsonObject{{"data_available", true},
                                               {"default_snapshot_id", "copy-1"}};
        service.state["availability"] = availability;
        MainWindow window(&service, true);
        window.show();
        service.start();
        GameRow *row = nullptr;
        for (auto *candidate : window.findChildren<GameRow *>())
            if (candidate->id == "void-war")
                row = candidate;
        QVERIFY(row);
        QCOMPARE(row->load->toolTip(),
                 "Folder modified: " + modified.addDays(1).toString("yyyy-MM-dd HH:mm:ss t"));
        QTest::mouseClick(row->arrow, Qt::LeftButton);
        auto *popup = window.findChild<QWidget *>("historyPopup");
        QVERIFY(popup);
        QTest::qWait(50);
        const auto historyRows = popup->findChildren<QWidget *>(QRegularExpression("historyRow-.*"));
        QVERIFY(!historyRows.isEmpty());
        QTest::mouseMove(historyRows.first(), historyRows.first()->rect().center());
        const auto screenshotRoot = QString(SOURCE_DIR) + "/build/desktop/screenshots/";
        QDir().mkpath(screenshotRoot);
        auto *historyBody = popup->findChild<QScrollArea *>()->widget();
        QVERIFY(historyBody->grab().save(screenshotRoot + "history-popup.png"));
        QStringList labels;
        for (auto *label : popup->findChildren<QLabel *>())
            labels.append(label->text());
        QVERIFY(labels.contains(Presentation::historyTime(modified.toMSecsSinceEpoch(),
                                                          QDateTime::currentDateTime())));
        QVERIFY(labels.contains(Presentation::historyTime(modified.addDays(1).toMSecsSinceEpoch(),
                                                          QDateTime::currentDateTime())));
        QVERIFY(labels.contains("Existing backup\nFolder modified"));
        QVERIFY(!labels.contains("Existing backup\nSave time unknown"));
        const auto actions = popup->findChildren<QPushButton *>("historyAction");
        QCOMPARE(actions.size(), 2);
        const auto times = popup->findChildren<QLabel *>("historyTime");
        QCOMPARE(times.size(), 2);
        QCOMPARE(times.first()->width(), times.last()->width());
        QVERIFY(times.first()->text().contains('\n'));
        auto *day = popup->findChild<QLabel *>("day");
        QVERIFY(day);
        QCOMPARE(times.first()->mapTo(popup, QPoint()).x(), day->mapTo(popup, QPoint()).x());
        const auto entryIcons = popup->findChildren<QLabel *>("historyEntryIcon");
        QCOMPARE(entryIcons.size(), 2);
        for (auto *icon : entryIcons)
            QVERIFY(!icon->pixmap().isNull());
        QVERIFY(actions.first()->isEnabled());
        QVERIFY(!actions.first()->icon().isNull());
        QVERIFY(actions.first()->toolTip().contains("current game data"));
        QVERIFY(actions.first()->toolTip() != QString("Restore"));
        QCOMPARE(actions.first()->property("checkpoint").toString(), QString("copy-1"));
        QTest::mouseClick(actions.first(), Qt::LeftButton);
        QCOMPARE(service.requests.last()["action"].toObject()["target"].toString(),
                 QString("copy-1"));
        const auto deletes = popup->findChildren<QPushButton *>("historyDelete");
        QCOMPARE(deletes.size(), 2);
        QCOMPARE(deletes.first()->size(), actions.first()->size());
        QVERIFY(!deletes.first()->icon().isNull());
        QVERIFY(deletes.first()->toolTip().contains("Permanently delete"));
        QVERIFY(deletes.first()->toolTip().contains("will not be changed"));
        QTest::mouseClick(deletes.first(), Qt::LeftButton);
        QCOMPARE(service.requests.last()["action"].toObject(),
                 (QJsonObject{{"type", "delete"}, {"target", "copy-1"}}));
    }
    void cachedSteamIconUpdatesWithoutChangingGameRevision() {
        QTemporaryDir temp;
        const auto path = temp.filePath("icon.png");
        QImage image(32, 32, QImage::Format_RGB32);
        image.fill(Qt::red);
        QVERIFY(image.save(path));
        auto state = demoState();
        GameRow row("void-war");
        row.updateState(state, true, true, false);
        auto *icon = row.findChild<QLabel *>("gameIcon");
        QVERIFY(icon);
        QCOMPARE(icon->text(), QString("VW"));
        const QJsonValue revision = state["revision"];
        state["artwork"] = QJsonObject{{"void-war", QJsonObject{{"steam_app_id", 2853590}, {"icon_path", path}}}};
        state["artwork_revision"] = 1;
        row.updateState(state, true, true, false);
        QVERIFY(!icon->pixmap().isNull());
        QCOMPARE(icon->size(), QSize(27, 27));
        QCOMPARE(state["revision"], revision);
        state["artwork"] = QJsonObject();
        state["artwork_revision"] = 2;
        row.updateState(state, true, true, false);
        QCOMPARE(icon->text(), QString("VW"));
        QVERIFY(icon->pixmap().isNull());
    }
    void localTransportReconnect() {
        QLocalServer server;
        const auto endpoint =
            "savescummer-test-" + QUuid::createUuid().toString(QUuid::WithoutBraces);
        QVERIFY(server.listen(endpoint));
        QList<QLocalSocket *> sockets;
        int watchCount = 0;
        int executeCount = 0;
        int artworkChecks = 0;
        QJsonValue watchRequestId;
        connect(&server, &QLocalServer::newConnection, &server, [&] {
            auto *socket = server.nextPendingConnection();
            sockets.append(socket);
            auto buffer = std::make_shared<QByteArray>();
            connect(socket, &QLocalSocket::readyRead, &server, [&, socket, buffer] {
                buffer->append(socket->readAll());
                for (const auto &request : Wire::consume(*buffer)) {
                    const auto command = request["command"].toObject();
                    if (command["type"] == "check_artwork") {
                        ++artworkChecks;
                        socket->write(Wire::frame({{"version", Wire::Version}, {"request_id", request["request_id"]},
                            {"host_id", "first"}, {"result", QJsonObject{{"type", "ok"}}}}));
                        continue;
                    }
                    if (command["type"] == "execute") {
                        ++executeCount;
                        socket->disconnectFromServer();
                        continue;
                    }
                    ++watchCount;
                    watchRequestId = request["request_id"];
                    auto state = demoState();
                    state["revision"] = watchCount == 1 ? 99 : 1;
                    const auto frame =
                        Wire::frame({{"version", Wire::Version},
                                     {"request_id", request["request_id"]},
                                     {"host_id", watchCount == 1 ? "first" : "restarted"},
                                     {"result", QJsonObject{{"type", "state"}, {"state", state}}}});
                    socket->write(frame.left(7));
                    socket->flush();
                    QTimer::singleShot(5, socket, [socket, frame] { socket->write(frame.mid(7)); });
                }
            });
        });
        LocalService service(endpoint, {}, {});
        QSignalSpy states(&service, &Service::stateChanged);
        service.start();
        QTRY_COMPARE(states.count(), 1);
        QTRY_COMPARE(artworkChecks, 1);
        auto artworkState = demoState();
        artworkState["revision"] = 99;
        artworkState["artwork_revision"] = 1;
        sockets.first()->write(Wire::frame({{"version", Wire::Version}, {"request_id", watchRequestId},
            {"host_id", "first"}, {"result", QJsonObject{{"type", "state"}, {"state", artworkState}}}}));
        QTRY_COMPARE(states.count(), 2);
        QCOMPARE(states.last()[0].toJsonObject()["artwork_revision"].toInteger(), qint64(1));
        bool replied = false;
        QJsonObject result;
        service.request({{"type", "execute"},
                         {"game_id", "void-war"},
                         {"action", QJsonObject{{"type", "save"}}}},
                        [&](const auto &reply) {
                            result = reply;
                            replied = true;
                        });
        QTRY_VERIFY(replied);
        QCOMPARE(result["type"].toString(), QString("error"));
        sockets.first()->disconnectFromServer();
        QTRY_COMPARE_WITH_TIMEOUT(states.count(), 3, 5000);
        QCOMPARE(states.last()[0].toJsonObject()["revision"].toInteger(), qint64(1));
        QCOMPARE(executeCount, 1);
        QTRY_COMPARE(artworkChecks, 2);
    }
    void renderScreenshots() {
        FakeService service;
        MainWindow window(&service, true);
        window.resize(620, 396);
        window.show();
        service.start();
        QTest::qWait(50);
        QDir().mkpath(QString(SOURCE_DIR) + "/build/desktop/screenshots");
        const auto root = QString(SOURCE_DIR) + "/build/desktop/screenshots/";
        QVERIFY(window.grab().save(root + "main.png"));
#ifdef Q_OS_WIN
        if (QGuiApplication::platformName() == "windows") {
            // Capture only our window, even when another app overlaps it.
            const auto handle = reinterpret_cast<HWND>(window.winId());
            RECT bounds;
            QVERIFY(GetWindowRect(handle, &bounds));
            const int width = bounds.right - bounds.left, height = bounds.bottom - bounds.top;
            BITMAPINFO info{};
            info.bmiHeader = {sizeof(BITMAPINFOHEADER), width, -height, 1, 32, BI_RGB};
            void *pixels = nullptr;
            const auto dc = CreateCompatibleDC(nullptr);
            const auto bitmap = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &pixels, nullptr, 0);
            const auto previous = SelectObject(dc, bitmap);
            const bool captured = PrintWindow(handle, dc, 2);
            const auto image =
                QImage(static_cast<uchar *>(pixels), width, height, QImage::Format_RGB32).copy();
            SelectObject(dc, previous);
            DeleteObject(bitmap);
            DeleteDC(dc);
            QVERIFY(captured);
            QVERIFY(image.save(root + "main-native.png"));
        }
#endif
        window.setOtherGamesOpen(true);
        window.resize(620, 550);
        QTest::qWait(30);
        QVERIFY(window.grab().save(root + "expanded.png"));
        window.selectGame("slay-the-spire");
        window.resize(620, 380);
        QTest::qWait(30);
        QVERIFY(window.grab().save(root + "selected.png"));
        window.resize(350, 520);
        QTest::qWait(30);
        QVERIFY(window.grab().save(root + "narrow.png"));
        for (auto *row : window.findChildren<GameRow *>())
            if (row->id == "slay-the-spire")
                QCOMPARE(row->save->width(), row->load->width() + row->arrow->width());
        MainWindow::applyTheme(false);
        QTest::qWait(30);
        QVERIFY(window.grab().save(root + "light.png"));
        MainWindow::applyTheme(true);
    }
    void realHostRoundTrip() {
        auto executable = qEnvironmentVariable("SAVESCUMMER_TEST_HOST");
#ifdef Q_OS_WIN
        if (executable.isEmpty())
            executable = QString(SOURCE_DIR) + "/target/debug/savescummer-host.exe";
#else
        if (executable.isEmpty())
            executable = QString(SOURCE_DIR) + "/target/debug/savescummer-host";
#endif
        if (!QFileInfo::exists(executable))
            QSKIP("Build savescummer-host to run the cross-language integration test.");
        QTemporaryDir temp;
        QVERIFY(temp.isValid());
        const auto root = temp.path() + "/state", live = temp.path() + "/Game";
        QVERIFY(QDir().mkpath(root));
        QVERIFY(QDir().mkpath(live));
        auto write = [&](const QByteArray &data) {
            QFile file(live + "/save.dat");
            if (!file.open(QIODevice::WriteOnly))
                return false;
            return file.write(data) == data.size();
        };
        QVERIFY(write("original"));
        QProcess host;
        host.start(executable, {"--data-dir", root, "--no-scan", "--no-monitor", "--no-audio", "--no-integrations", "--no-artwork"});
        QVERIFY(host.waitForStarted());
        QVERIFY(host.waitForReadyRead(15000));
        QVERIFY(host.readAllStandardOutput().contains("\"ready\":true"));
        LocalService service(Wire::endpoint(root), {}, {});
        QSignalSpy updates(&service, &Service::stateChanged);
        service.start();
        QTRY_VERIFY(!updates.isEmpty());
        auto send = [&](LocalService &client, const QJsonObject &command) {
            auto result = std::make_shared<QJsonObject>();
            QEventLoop loop;
            client.request(command, [result, &loop](const auto &reply) {
                *result = reply;
                loop.quit();
            });
            loop.exec();
            return *result;
        };
        auto reply = send(service, {{"type", "configure"},
                                    {"id", "test"},
                                    {"name", "Test game"},
                                    {"data_dir", live},
                                    {"executables", QJsonArray()}});
        QCOMPARE(reply["type"].toString(), QString("configured"));
        reply = send(service, {{"type", "state"}});
        auto state = reply["state"].toObject();
        QVERIFY(state["availability"].toObject()["test"].toObject()["data_available"].toBool());
        QVERIFY(
            state["availability"].toObject()["test"].toObject()["default_snapshot_id"].isNull());
        reply = send(
            service,
            {{"type", "execute"}, {"game_id", "test"}, {"action", QJsonObject{{"type", "save"}}}});
        QCOMPARE(reply["type"].toString(), QString("accepted"));
        const auto saveId = reply["operation_id"].toString();
        QTRY_COMPARE_WITH_TIMEOUT(updates.last()[0]
                                      .toJsonObject()["operations"]
                                      .toObject()[saveId]
                                      .toObject()["status"]
                                      .toString(),
                                  QString("completed"), 10000);
        state = updates.last()[0].toJsonObject();
        QVERIFY(
            state["availability"].toObject()["test"].toObject()["default_snapshot_id"].isString());
        const QJsonValue checkpoint =
            state["availability"].toObject()["test"].toObject()["default_snapshot_id"];
        QVERIFY(write("changed"));
        reply = send(service, {{"type", "execute"},
                               {"game_id", "test"},
                               {"action", QJsonObject{{"type", "load"}, {"target", checkpoint}}}});
        QCOMPARE(reply["type"].toString(), QString("accepted"));
        const auto loadId = reply["operation_id"].toString();
        QTRY_COMPARE_WITH_TIMEOUT(updates.last()[0]
                                      .toJsonObject()["operations"]
                                      .toObject()[loadId]
                                      .toObject()["status"]
                                      .toString(),
                                  QString("completed"), 10000);
        QFile restored(live + "/save.dat");
        QVERIFY(restored.open(QIODevice::ReadOnly));
        QCOMPARE(restored.readAll(), QByteArray("original"));
        restored.close();
        // New Qt client retrieves durable operation state without replaying it.
        LocalService reopened(Wire::endpoint(root), {}, {});
        QSignalSpy reopenedStates(&reopened, &Service::stateChanged);
        reopened.start();
        QTRY_VERIFY(!reopenedStates.isEmpty());
        reply = send(reopened, {{"type", "operation"}, {"operation_id", loadId}});
        QCOMPARE(reply["operation"].toObject()["status"].toString(), QString("completed"));
        reply = send(reopened, {{"type","history"},{"game_id","test"},{"limit",50}});
        QCOMPARE(reply["type"].toString(),QString("history_page"));
        QVERIFY(!reopenedStates.last()[0].toJsonObject().contains("history"));
        const QJsonValue recovery = reply["page"].toObject()["rows"].toArray().first().toObject()["recovery_id"];
        reply = send(reopened, {{"type", "execute"},
                                {"game_id", "test"},
                                {"action", QJsonObject{{"type", "revert"}, {"target", recovery}}}});
        QCOMPARE(reply["type"].toString(), QString("accepted"));
        const auto revertId = reply["operation_id"].toString();
        QTRY_COMPARE_WITH_TIMEOUT(reopenedStates.last()[0]
                                      .toJsonObject()["operations"]
                                      .toObject()[revertId]
                                      .toObject()["status"]
                                      .toString(),
                                  QString("completed"), 10000);
        QVERIFY(restored.open(QIODevice::ReadOnly));
        QCOMPARE(restored.readAll(), QByteArray("changed"));
        restored.close();
        // A missing live directory disables Save even while backups survive.
        QVERIFY(QDir().rename(live, temp.path() + "/Detached"));
        QTRY_VERIFY_WITH_TIMEOUT(!reopenedStates.last()[0]
                                      .toJsonObject()["availability"]
                                      .toObject()["test"]
                                      .toObject()["data_available"]
                                      .toBool(),
                                 10000);
        QSignalSpy stopping(&reopened, &Service::hostStopping);
        QCOMPARE(send(reopened, {{"type", "shutdown"}})["type"].toString(), QString("ok"));
        QTRY_COMPARE(stopping.size(), 1);
        QVERIFY(host.state() == QProcess::NotRunning || host.waitForFinished(10000));
        QCOMPARE(host.exitStatus(), QProcess::NormalExit);
        QCOMPARE(host.exitCode(), 0);
    }
};
QTEST_MAIN(DesktopTest)
#include "desktop_test.moc"
