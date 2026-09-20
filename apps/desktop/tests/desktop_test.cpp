#include "demo.h"
#include "mainwindow.h"
#include "presentation.h"
#include <QApplication>
#include <QEventLoop>
#include <QFile>
#include <QFontDatabase>
#include <QJsonDocument>
#include <QLocalServer>
#include <QProcess>
#include <QScreen>
#include <QSignalSpy>
#include <QTemporaryDir>
#include <QTest>
#include <QTextDocument>
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
    QJsonObject executeReply{
        {"type", "error"}, {"error", QJsonObject{{"code", "busy"}, {"message", "Game is busy."}}}};
    void start() override {
        emit connectionChanged(true, {});
        emit stateChanged(state);
    }
    void request(const QJsonObject &command, Callback callback = {}) override {
        requests.append(command);
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
                         : QJsonObject{{"type", "state"}, {"state", state}});
    }
    void publish() { emit stateChanged(state); }
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
        for (const auto &name : {"state", "save", "load", "revert", "sounds", "startup", "active", "explorer", "reset", "artwork"}) {
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
                QVERIFY(row->more->isEnabled());
                QTest::keyClick(row->header, Qt::Key_Space);
                QCOMPARE(window.selectedGame(), row->id);
            }
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
        QVERIFY(action);
        QVERIFY(!action->isEnabled());
        snapshot["available"] = true;
        snapshots["void-war-saved"] = snapshot;
        service.state["snapshots"] = snapshots;
        service.publish();
        action = window.findChild<QPushButton *>("historyAction");
        QVERIFY(action->isEnabled());
        QTest::mouseClick(action, Qt::LeftButton);
        QCOMPARE(service.requests.last()["action"].toObject()["target"].toString(),
                 QString("void-war-saved"));
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
                        socket->write(Wire::frame({{"version", 2}, {"request_id", request["request_id"]},
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
                        Wire::frame({{"version", 2},
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
        sockets.first()->write(Wire::frame({{"version", 2}, {"request_id", watchRequestId},
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
        const QJsonValue recovery =
            Presentation::history(reopenedStates.last()[0].toJsonObject(), "test")
                .first()["recovery_id"];
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
