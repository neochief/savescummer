#pragma once
#include "service.h"
#include <QCheckBox>
#include <QGridLayout>
#include <QLabel>
#include <QJsonArray>
#include <QMainWindow>
#include <QProgressBar>
#include <QPushButton>
#include <QScrollArea>
#include <QVBoxLayout>

class LoadButton : public QPushButton {
  public:
    using QPushButton::QPushButton;
    void setAge(const QString &age, const QString &full);
    QSize sizeHint() const override;
    bool hasHeightForWidth() const override { return true; }
    int heightForWidth(int width) const override;

  protected:
    void paintEvent(QPaintEvent *) override;

  private:
    QFont captionFont() const;
    QString age_;
};

class GameRow : public QWidget {
    Q_OBJECT
  public:
    explicit GameRow(const QString &id, QWidget *parent = nullptr);
    void updateState(const QJsonObject &state, bool selected, bool connected, bool submitting);
    QString id;
    QPushButton *header, *save, *arrow, *more, *recovery;
    LoadButton *load;
    QLabel *info, *error;
    QProgressBar *progress;
  signals:
    void selected();
    void action(const QJsonObject &action);
    void showHistory();
    void showOptions();
    void showRecovery();

  protected:
    void mousePressEvent(QMouseEvent *) override;
    bool eventFilter(QObject *, QEvent *) override;
    void resizeEvent(QResizeEvent *) override;

  private:
    void arrangeControls();
    QLabel *icon_, *status_;
    QWidget *details_, *controls_, *pair_, *split_;
    bool selected_ = false;
};

class MainWindow : public QMainWindow {
    Q_OBJECT
  public:
    explicit MainWindow(Service *service, bool demo = false, QWidget *parent = nullptr);
    QString selectedGame() const { return selected_; }
    void applyState(const QJsonObject &state);
    void selectGame(const QString &id);
    void setOtherGamesOpen(bool open);
    void setConnected(bool connected, const QString &message = {});
    static void applyTheme(bool dark);

  protected:
    void resizeEvent(QResizeEvent *event) override;
#ifdef Q_OS_WIN
    bool nativeEvent(const QByteArray &eventType, void *message, qintptr *result) override;
#endif

  private:
    void triggerSelectedShortcut(bool load);
    void arrangeOtherHeader();
    void scheduleFitHeight();
    void refresh();
    void closePopup();
    void execute(const QString &game, const QJsonObject &action);
    void history(const QString &game);
    void populateHistory();
    void fetchHistory(bool older = false, bool preserveAnchor = false);
    void options(const QString &game);
    void configure(const QString &game);
    void addCustomGame();
    void flush(const QString &game, bool forget = false);
    void recover(const QString &game);
    void showError(const QString &game, const QString &message);
    void placePopup(QWidget *popup, QWidget *anchor);
    Service *service_;
    QJsonObject state_;
    QMap<QString, GameRow *> rows_;
    QMap<QString, QString> errors_;
    QSet<QString> submitting_;
    QString selected_, historyGame_;
    bool connected_ = false, othersOpen_ = false, hadRunning_ = false;
    bool groupInitialized_ = false, scanPending_ = false;
    bool resetRevision_ = true;
    bool soundSettingPending_ = false;
    QCheckBox *sounds_;
    QCheckBox *startup_;
    bool startupSettingPending_ = false;
    QLabel *empty_;
    QWidget *otherHeader_, *otherActions_;
    QPushButton *otherToggle_, *scanGames_, *addGame_;
    QGridLayout *otherHeaderLayout_;
    QVBoxLayout *gamesLayout_;
    QWidget *games_;
    QScrollArea *scroll_;
    QWidget *footer_;
    bool fitQueued_ = false;
    int lastContentHeight_ = -1;
    int lastFitWidth_ = -1;
    QString pendingSelection_;
    QPointer<QWidget> popup_;
    QPointer<QWidget> historyBody_;
    QJsonArray historyRows_;
    QString historyCursor_;
    qint64 historyRevision_ = -1;
    quint64 historyRequest_ = 0;
    bool historyLoading_ = false;
    QPointer<QScrollArea> historyScroll_;
};
